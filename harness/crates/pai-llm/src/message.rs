//! Conversation vocabulary: what goes *into* a request and what is assembled *from* a stream.
//! Deliberately provider-neutral, each adapter translating into its own protocol, because
//! extension plugins may depend on seam definitions but never on a concrete provider.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::LlmError;

/// Identifies a tool call. OpenAI emits its own string; Ollama emits none, so the adapter makes one.
pub type ToolCallId = String;

/// One piece of content in a message; `ToolUse.arguments` stays a raw JSON string, not a `Value`, because it arrives in fragments and is not valid JSON until the stream closes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// Surfaced reasoning (`thinking` on Ollama, `reasoning_content` elsewhere); separate from `Text` because the UI shows it differently and it is not sent back next round.
    Reasoning {
        text: String,
    },
    ToolUse {
        id: ToolCallId,
        name: String,
        /// Raw JSON string. See the enum note.
        arguments: String,
    },
    /// A tool result in carryable form; on the wire it is its own role, but the session log keeps call and result side by side.
    ToolResult {
        tool_call_id: ToolCallId,
        content: String,
        is_error: bool,
    },
    /// Base64-encoded image; kept as base64 rather than a path because the two adapters need different shapes and both build from base64 without touching disk again.
    Image {
        mime: String,
        data: String,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning { text: text.into() }
    }

    pub fn tool_use(
        id: impl Into<ToolCallId>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self::ToolUse {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    pub fn image(mime: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Image {
            mime: mime.into(),
            data: data.into(),
        }
    }

    /// The block's text, if any. `Reasoning` does not count: it is not the answer.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// A finished tool call, lifted out of `ContentBlock` so callers need no `match`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    /// Raw JSON string exactly as the model emitted it.
    pub arguments: String,
}

impl ToolCall {
    /// Parse the arguments - the only place allowed to; it returns `Result` because a small model emitting broken JSON is routine and the agent loop must report it back in text.
    pub fn parse_arguments(&self) -> Result<Value, LlmError> {
        let raw = self.arguments.trim();
        if raw.is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        serde_json::from_str(raw).map_err(|err| {
            LlmError::invalid(format!(
                "tool `{}` phát ra JSON tham số không hợp lệ: {err}",
                self.name
            ))
        })
    }
}

/// One turn in the conversation; `System` holds a `String`, since no provider accepts images in the system role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: Vec<ContentBlock>,
    },
    Assistant {
        content: Vec<ContentBlock>,
    },
    Tool {
        tool_call_id: ToolCallId,
        /// Tool name. Ollama needs it on the wire (`tool_name`); OpenAI does not.
        name: String,
        content: String,
        is_error: bool,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: vec![ContentBlock::text(content)],
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: vec![ContentBlock::text(content)],
        }
    }

    pub fn tool(
        tool_call_id: impl Into<ToolCallId>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self::Tool {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            content: content.into(),
            is_error: false,
        }
    }

    /// Turn a logged `ToolResult` block into a wire `tool` role message; `name` must come from the matching call, which the result block does not carry.
    pub fn from_tool_result(block: &ContentBlock, name: impl Into<String>) -> Option<Self> {
        match block {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => Some(Self::Tool {
                tool_call_id: tool_call_id.clone(),
                name: name.into(),
                content: content.clone(),
                is_error: *is_error,
            }),
            _ => None,
        }
    }

    /// All answer text in the message, concatenated. Skips reasoning and tool calls.
    pub fn text(&self) -> String {
        match self {
            Self::System { content } => content.clone(),
            Self::Tool { content, .. } => content.clone(),
            Self::User { content } | Self::Assistant { content } => {
                content.iter().filter_map(ContentBlock::as_text).collect()
            }
        }
    }

    /// Every tool call in the message, in order.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        let Self::Assistant { content } = self else {
            return Vec::new();
        };
        content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse {
                    id,
                    name,
                    arguments,
                } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}

/// Describes a tool to the model; `parameters` stays a raw JSON Schema, because MCP delivers JSON and round-tripping it through Rust types would shave off unknown keywords.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSchema {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

/// A request to the model; deliberately carries no `provider` (the adapter knows) and no cancel token (cancelling means dropping the stream).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Vec<String>,
    /// Ollama-only keep-alive hint (`"5m"` to hold VRAM, `"0"` to release); the OpenAI adapter ignores it rather than erroring.
    pub keep_alive: Option<String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Self::default()
        }
    }

    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_temperature(mut self, value: f32) -> Self {
        self.temperature = Some(value);
        self
    }

    pub fn with_max_tokens(mut self, value: u32) -> Self {
        self.max_tokens = Some(value);
        self
    }

    pub fn with_keep_alive(mut self, value: impl Into<String>) -> Self {
        self.keep_alive = Some(value.into());
        self
    }
}
