//! OpenAI-compatible adapter: `/v1/chat/completions` over SSE.
//! Targets llama.cpp, vLLM, LM Studio and real OpenAI, so it sends the smallest common
//! denominator - every extra field is some server returning 400. Tool args arrive fragmented.

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::BoxStream;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use crate::capabilities::Capabilities;
use crate::error::{LlmError, LlmErrorCode};
use crate::message::{ChatRequest, ContentBlock, Message};
use crate::model::{ModelInfo, ModelState};
use crate::seam::LlmAdapter;
use crate::stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};
use crate::wire::pump::{FrameDecoder, pump};
use crate::wire::{SseDecoder, SseEvent};

/// Normalize the base URL, accepting an API root or a bare host: a `v<n>` suffix is kept, otherwise `/v1` is appended.
pub fn openai_base_url(base_url: &str) -> Result<String, LlmError> {
    let value = base_url.trim().trim_end_matches('/');
    if value.is_empty() {
        return Err(LlmError::new(
            LlmErrorCode::NoProviderConfigured,
            "Provider chưa có địa chỉ máy chủ",
        ));
    }
    let tail = value.rsplit('/').next().unwrap_or_default();
    let versioned =
        tail.starts_with('v') && tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit());
    Ok(if versioned {
        value.to_string()
    } else {
        format!("{value}/v1")
    })
}

/// Talks to a server speaking the OpenAI protocol.
pub struct OpenAiAdapter {
    id: String,
    api_root: String,
    api_key: String,
    http: reqwest::Client,
}

impl OpenAiAdapter {
    /// `base_url` accepts both `http://host:port` and `http://host:port/v1`.
    pub fn new(
        id: impl Into<String>,
        base_url: &str,
        api_key: impl Into<String>,
        http: reqwest::Client,
    ) -> Result<Self, LlmError> {
        Ok(Self {
            id: id.into(),
            api_root: openai_base_url(base_url)?,
            api_key: api_key.into(),
            http,
        })
    }

    pub fn api_root(&self) -> &str {
        &self.api_root
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            // Drop the header entirely rather than sending a placeholder key: protocol-correct, and no fake string reaches the logs.
            return builder;
        }
        builder.bearer_auth(&self.api_key)
    }

    /// `/v1/models`. Not part of the `ModelAdmin` seam: a remote server keeps models elsewhere, so this is a list to read, not a lifecycle to drive.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let response = self
            .authorized(self.http.get(format!("{}/models", self.api_root)))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status.as_u16(), &body));
        }
        let payload: Value = serde_json::from_str(&response.text().await?)?;
        let Some(entries) = payload.get("data").and_then(Value::as_array) else {
            return Err(LlmError::invalid(
                "Provider trả về danh sách mô hình không hợp lệ",
            ));
        };
        let mut models: Vec<ModelInfo> = entries
            .iter()
            .filter_map(|entry| {
                let name = entry.get("id").and_then(Value::as_str)?.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                let owner = entry
                    .get("owned_by")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(ModelInfo {
                    // `/v1/models` returns only id and `owned_by`, so guessing is the only branch left.
                    capabilities: Capabilities::infer(&format!("{name} {owner}")),
                    name,
                    state: ModelState::Installed,
                    size_bytes: 0,
                    vram_bytes: 0,
                    quantization: None,
                    modified_at: None,
                })
            })
            .collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }
}

#[async_trait]
impl LlmAdapter for OpenAiAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        let body = encode_chat(&req);
        let builder = self.authorized(
            self.http
                .post(format!("{}/chat/completions", self.api_root)),
        );
        let request =
            async move { builder.json(&body).send().await.map_err(LlmError::from) }.boxed();
        pump(request, ChatDecoder::new())
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        // There is no `/api/show` on this side; the OpenAI protocol declares no capabilities, so guessing is the only source and `source` says so.
        Ok(Capabilities::infer(model))
    }

    async fn health(&self) -> bool {
        self.authorized(self.http.get(format!("{}/models", self.api_root)))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }
}

/// Build the `/v1/chat/completions` request body.
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
    if let Some(temperature) = req.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if let Some(max_tokens) = req.max_tokens {
        body.insert("max_tokens".into(), json!(max_tokens));
    }
    if !req.stop.is_empty() {
        body.insert("stop".into(), json!(req.stop));
    }
    // `keep_alive` is deliberately not sent - it is an Ollama concept - and ignoring beats erroring, since callers build one `ChatRequest` for every provider.
    Value::Object(body)
}

fn encode_message(message: &Message) -> Value {
    match message {
        Message::System { content } => json!({ "role": "system", "content": content }),
        Message::User { content } => {
            let has_image = content
                .iter()
                .any(|b| matches!(b, ContentBlock::Image { .. }));
            if !has_image {
                // Plain string, not an array: llama.cpp and some older vLLM builds accept only this. Upgrade to an array only for real images.
                return json!({ "role": "user", "content": joined_text(content) });
            }
            let parts: Vec<Value> = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(json!({ "type": "text", "text": text })),
                    ContentBlock::Image { mime, data } => Some(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{mime};base64,{data}") }
                    })),
                    _ => None,
                })
                .collect();
            json!({ "role": "user", "content": parts })
        }
        Message::Assistant { content } => {
            let mut object = Map::new();
            object.insert("role".into(), json!("assistant"));
            let text = joined_text(content);
            let calls: Vec<Value> = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse {
                        id,
                        name,
                        arguments,
                    } => Some(json!({
                        "id": id,
                        "type": "function",
                        // In this direction `arguments` *is* a string per the protocol, so keep exactly what the model emitted.
                        "function": { "name": name, "arguments": arguments },
                    })),
                    _ => None,
                })
                .collect();
            if calls.is_empty() {
                object.insert("content".into(), json!(text));
            } else {
                // `content` must be present and `null` when empty: an empty string makes some servers read this as an answer turn rather than a tool call.
                object.insert(
                    "content".into(),
                    if text.is_empty() {
                        Value::Null
                    } else {
                        json!(text)
                    },
                );
                object.insert("tool_calls".into(), Value::Array(calls));
            }
            Value::Object(object)
        }
        Message::Tool {
            tool_call_id,
            content,
            ..
        } => {
            json!({ "role": "tool", "tool_call_id": tool_call_id, "content": content })
        }
    }
}

fn joined_text(blocks: &[ContentBlock]) -> String {
    blocks.iter().filter_map(ContentBlock::as_text).collect()
}

/// SSE decoder for `/v1/chat/completions`.
#[derive(Debug, Default)]
pub struct ChatDecoder {
    sse: SseDecoder,
    text_index: Option<u32>,
    reasoning_index: Option<u32>,
    /// Maps OpenAI's `tool_calls[].index` to our block `index`; the two sequences are independent.
    tool_blocks: BTreeMap<u64, u32>,
    next_index: u32,
    usage: Option<TokenUsage>,
    reason: Option<FinishReason>,
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

    fn event(&mut self, event: &SseEvent, out: &mut Vec<StreamChunk>) -> Result<(), LlmError> {
        if event.is_done() {
            self.terminate(out);
            return Ok(());
        }
        let Ok(value) = serde_json::from_str::<Value>(event.data.trim()) else {
            tracing::warn!(data = %event.data, "unreadable SSE event, skipping");
            return Ok(());
        };
        if let Some(message) = value
            .get("error")
            .and_then(|err| err.get("message").or(Some(err)))
            .and_then(Value::as_str)
        {
            return Err(LlmError::unavailable(message.to_string()));
        }
        if let Some(usage) = value.get("usage").and_then(Value::as_object) {
            self.usage = Some(TokenUsage {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                output_tokens: usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
            });
        }
        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        else {
            return Ok(());
        };
        if let Some(delta) = choice.get("delta") {
            self.delta(delta, out);
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.reason = Some(match reason {
                "tool_calls" | "function_call" => FinishReason::ToolCalls,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::ContentFilter,
                _ => FinishReason::Stop,
            });
        }
        Ok(())
    }

    fn delta(&mut self, delta: &Value, out: &mut Vec<StreamChunk>) {
        // DeepSeek and some vLLM builds put reasoning in `reasoning_content`, newer ones in `reasoning`; accept both.
        if let Some(text) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
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
        if let Some(text) = delta.get("content").and_then(Value::as_str)
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
        let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            return;
        };
        for (position, call) in calls.iter().enumerate() {
            // `index` is required by the spec, but some servers omit it for a single tool call; fall back to the array position.
            let slot = call
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(position as u64);
            let (index, fresh) = match self.tool_blocks.get(&slot) {
                Some(index) => (*index, false),
                None => {
                    let index = self.allocate();
                    self.tool_blocks.insert(slot, index);
                    (index, true)
                }
            };
            if fresh {
                out.push(StreamChunk::BlockStart {
                    index,
                    kind: BlockKind::ToolUse,
                });
            }
            let function = call.get("function");
            out.push(StreamChunk::ToolCallDelta {
                index,
                id: call.get("id").and_then(Value::as_str).map(str::to_string),
                name: function
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                // A JSON string fragment: may be empty, may split an escape. Pass it through.
                arguments: function
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }

    /// Close every open block, then emit `Usage` and `Finish` in that invariant order.
    fn terminate(&mut self, out: &mut Vec<StreamChunk>) {
        if self.finished {
            return;
        }
        if let Some(index) = self.reasoning_index.take() {
            out.push(StreamChunk::BlockEnd { index });
        }
        if let Some(index) = self.text_index.take() {
            out.push(StreamChunk::BlockEnd { index });
        }
        for index in std::mem::take(&mut self.tool_blocks).into_values() {
            out.push(StreamChunk::BlockEnd { index });
        }
        if let Some(usage) = self.usage.take() {
            out.push(StreamChunk::Usage { usage });
        }
        out.push(StreamChunk::Finish {
            reason: self.reason.unwrap_or(FinishReason::Stop),
        });
        self.finished = true;
    }
}

impl FrameDecoder for ChatDecoder {
    fn push(&mut self, bytes: &[u8], out: &mut Vec<StreamChunk>) -> Result<(), LlmError> {
        for event in self.sse.push(bytes) {
            if self.finished {
                break;
            }
            self.event(&event, out)?;
        }
        Ok(())
    }

    fn finish(&mut self, out: &mut Vec<StreamChunk>) {
        if let Some(event) = self.sse.flush()
            && !self.finished
            && let Err(err) = self.event(&event, out)
        {
            tracing::warn!(%err, "final SSE event is broken");
        }
        // No `[DONE]` but a `finish_reason` was seen: the server closed early yet said enough, so close cleanly; with nothing seen, let `pump` report a truncated answer.
        if !self.finished && self.reason.is_some() {
            self.terminate(out);
        }
    }

    fn saw_finish(&self) -> bool {
        self.finished
    }
}
