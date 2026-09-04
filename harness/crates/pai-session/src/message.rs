//! Minimal message vocabulary, owned long-term by `pai-llm` but declared here because the projection
//! must read exactly one thing: whether a message has empty content. Beyond that the log never
//! interprets content -- framing belongs to the producer, or replay stops matching what the model saw.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        /// Unparsed JSON string: the model produced these exact bytes, and the bytes are what must replay.
        arguments: String,
    },
    ToolResult {
        call_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    /// Who produced this message when it is not the user (`subagent-report`, `compaction-checkpoint`); UI-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Message {
    pub fn text(role: Role, text: impl Into<String>) -> Message {
        Message {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
            source: None,
        }
    }

    pub fn user(text: impl Into<String>) -> Message {
        Message::text(Role::User, text)
    }

    pub fn assistant(text: impl Into<String>) -> Message {
        Message::text(Role::Assistant, text)
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}
