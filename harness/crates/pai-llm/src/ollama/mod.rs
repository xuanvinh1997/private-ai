//! Ollama adapter: `/api/chat` as NDJSON, not SSE - one JSON object per line, the last
//! carrying `"done": true` and the stats. Ollama also sends tool calls whole, so this
//! adapter emits one `ToolCallDelta` holding the entire string to keep the assembler simple.

pub mod admin;

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::BoxStream;
use serde_json::{Map, Value, json};

use crate::capabilities::Capabilities;
use crate::error::LlmError;
use crate::message::{ChatRequest, ContentBlock, Message};
use crate::seam::{LlmAdapter, ModelAdmin};
use crate::stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};
use crate::wire::LineDecoder;
use crate::wire::pump::{FrameDecoder, pump};

pub use admin::OllamaAdmin;

/// Talks to an Ollama server.
pub struct OllamaAdapter {
    id: String,
    base_url: String,
    http: reqwest::Client,
    admin: Arc<OllamaAdmin>,
}

impl OllamaAdapter {
    /// `base_url` is the server root (`http://localhost:11434`), not `/api`.
    pub fn new(id: impl Into<String>, base_url: impl AsRef<str>, http: reqwest::Client) -> Self {
        let base_url = base_url.as_ref().trim_end_matches('/').to_string();
        let admin = Arc::new(OllamaAdmin::new(base_url.clone(), http.clone()));
        Self {
            id: id.into(),
            base_url,
            http,
            admin,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[async_trait]
impl LlmAdapter for OllamaAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        let url = format!("{}/api/chat", self.base_url);
        let http = self.http.clone();
        let body = encode_chat(&req);
        let request = async move {
            http.post(url)
                .json(&body)
                .send()
                .await
                .map_err(LlmError::from)
        }
        .boxed();
        pump(request, ChatDecoder::new())
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        // Order matters: ask the server first, guess by name only if `/api/show` cannot answer.
        Ok(self
            .admin
            .show(model)
            .await
            .map(|details| details.capabilities)
            .unwrap_or_else(|_| Capabilities::infer(model)))
    }

    async fn health(&self) -> bool {
        self.admin.health().await
    }

    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        Some(self.admin.clone())
    }
}

/// Build the `/api/chat` request body.
pub(crate) fn encode_chat(req: &ChatRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert(
        "messages".into(),
        Value::Array(req.messages.iter().map(encode_message).collect()),
    );
    body.insert("stream".into(), json!(true));

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
    }

    // `keep_alive` is the only model load/unload control Ollama gives us: `"5m"` to stay warm, `"0"` to release.
    if let Some(keep_alive) = &req.keep_alive {
        body.insert("keep_alive".into(), json!(keep_alive));
    }

    // Ollama's sampling parameters live in `options`, not at the top level as with OpenAI.
    let mut options = Map::new();
    if let Some(temperature) = req.temperature {
        options.insert("temperature".into(), json!(temperature));
    }
    if let Some(max_tokens) = req.max_tokens {
        options.insert("num_predict".into(), json!(max_tokens));
    }
    if !req.stop.is_empty() {
        options.insert("stop".into(), json!(req.stop));
    }
    if !options.is_empty() {
        body.insert("options".into(), Value::Object(options));
    }
    Value::Object(body)
}

fn encode_message(message: &Message) -> Value {
    match message {
        Message::System { content } => json!({ "role": "system", "content": content }),
        Message::User { content } => {
            let mut object = Map::new();
            object.insert("role".into(), json!("user"));
            object.insert("content".into(), json!(joined_text(content)));
            // Ollama takes images as plain base64 in an array, with no `data:` prefix.
            let images: Vec<Value> = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Image { data, .. } => Some(json!(data)),
                    _ => None,
                })
                .collect();
            if !images.is_empty() {
                object.insert("images".into(), Value::Array(images));
            }
            Value::Object(object)
        }
        Message::Assistant { content } => {
            let mut object = Map::new();
            object.insert("role".into(), json!("assistant"));
            // Reasoning is not sent back: it is last round's scratch, and replaying it costs tokens and makes the model repeat itself.
            object.insert("content".into(), json!(joined_text(content)));
            let calls: Vec<Value> = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse {
                        name, arguments, ..
                    } => Some(json!({
                        "function": {
                            "name": name,
                            // Ollama wants an object, not a string; on broken arguments send the raw string so the server refuses loudly.
                            "arguments": serde_json::from_str::<Value>(arguments)
                                .unwrap_or_else(|_| Value::String(arguments.clone())),
                        }
                    })),
                    _ => None,
                })
                .collect();
            if !calls.is_empty() {
                object.insert("tool_calls".into(), Value::Array(calls));
            }
            Value::Object(object)
        }
        Message::Tool { name, content, .. } => {
            // Ollama matches results to calls by *name*, not id, since it never emits one; `tool_call_id` stays in the vocabulary for OpenAI.
            json!({ "role": "tool", "tool_name": name, "content": content })
        }
    }
}

fn joined_text(blocks: &[ContentBlock]) -> String {
    blocks.iter().filter_map(ContentBlock::as_text).collect()
}

/// NDJSON decoder for `/api/chat`.
#[derive(Debug, Default)]
pub struct ChatDecoder {
    lines: LineDecoder,
    text_index: Option<u32>,
    reasoning_index: Option<u32>,
    next_index: u32,
    saw_tool_call: bool,
    finished: bool,
}

impl ChatDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate(&mut self) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        index
    }

    fn line(&mut self, line: &str, out: &mut Vec<StreamChunk>) -> Result<(), LlmError> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                // A bad line breaks only that line; Ollama sometimes emits junk when the server reloads mid-stream.
                tracing::warn!(%err, line = trimmed, "unreadable NDJSON line, skipping");
                return Ok(());
            }
        };

        // Ollama reports errors *inside* the stream, with HTTP 200 on the outside.
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            return Err(LlmError::unavailable(message.to_string()));
        }

        if let Some(message) = value.get("message") {
            if let Some(text) = message.get("thinking").and_then(Value::as_str)
                && !text.is_empty()
            {
                let index = match self.reasoning_index {
                    Some(index) => index,
                    None => {
                        let index = self.allocate();
                        self.reasoning_index = Some(index);
                        out.push(StreamChunk::BlockStart {
                            index,
                            kind: BlockKind::Reasoning,
                        });
                        index
                    }
                };
                out.push(StreamChunk::ReasoningDelta {
                    index,
                    text: text.to_string(),
                });
            }
            if let Some(text) = message.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                let index = match self.text_index {
                    Some(index) => index,
                    None => {
                        let index = self.allocate();
                        self.text_index = Some(index);
                        out.push(StreamChunk::BlockStart {
                            index,
                            kind: BlockKind::Text,
                        });
                        index
                    }
                };
                out.push(StreamChunk::TextDelta {
                    index,
                    text: text.to_string(),
                });
            }
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let Some(function) = call.get("function") else {
                        continue;
                    };
                    let name = function
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name.is_empty() {
                        continue;
                    }
                    let arguments = match function.get("arguments") {
                        Some(Value::String(raw)) => raw.clone(),
                        Some(value) => value.to_string(),
                        None => "{}".to_string(),
                    };
                    let index = self.allocate();
                    self.saw_tool_call = true;
                    out.push(StreamChunk::BlockStart {
                        index,
                        kind: BlockKind::ToolUse,
                    });
                    out.push(StreamChunk::ToolCallDelta {
                        index,
                        // Ollama emits no id; the assembler mints `call_<index>` so the next round has something to match.
                        id: call.get("id").and_then(Value::as_str).map(str::to_string),
                        name: Some(name.to_string()),
                        arguments,
                    });
                    out.push(StreamChunk::BlockEnd { index });
                }
            }
        }

        if value.get("done").and_then(Value::as_bool).unwrap_or(false) {
            self.close(out);
            let usage = TokenUsage {
                input_tokens: value
                    .get("prompt_eval_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                output_tokens: value.get("eval_count").and_then(Value::as_u64).unwrap_or(0),
                total_tokens: None,
            };
            if usage.input_tokens > 0 || usage.output_tokens > 0 {
                out.push(StreamChunk::Usage { usage });
            }
            let reason = if self.saw_tool_call {
                // Ollama reports `done_reason: "stop"` even when it just asked for a tool, so that field alone cannot drive the agent loop.
                FinishReason::ToolCalls
            } else {
                match value.get("done_reason").and_then(Value::as_str) {
                    Some("length") => FinishReason::Length,
                    _ => FinishReason::Stop,
                }
            };
            out.push(StreamChunk::Finish { reason });
            self.finished = true;
        }
        Ok(())
    }

    fn close(&mut self, out: &mut Vec<StreamChunk>) {
        if let Some(index) = self.reasoning_index.take() {
            out.push(StreamChunk::BlockEnd { index });
        }
        if let Some(index) = self.text_index.take() {
            out.push(StreamChunk::BlockEnd { index });
        }
    }
}

impl FrameDecoder for ChatDecoder {
    fn push(&mut self, bytes: &[u8], out: &mut Vec<StreamChunk>) -> Result<(), LlmError> {
        for line in self.lines.push(bytes) {
            if self.finished {
                break;
            }
            self.line(&line, out)?;
        }
        Ok(())
    }

    fn finish(&mut self, out: &mut Vec<StreamChunk>) {
        if self.finished {
            return;
        }
        // The server closed without a trailing `\n`: the `done` line is still buffered.
        if let Some(rest) = self.lines.flush()
            && let Err(err) = self.line(&rest, out)
        {
            tracing::warn!(%err, "final NDJSON line is broken");
        }
    }

    fn saw_finish(&self) -> bool {
        self.finished
    }
}
