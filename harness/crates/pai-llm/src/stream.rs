//! Stream vocabulary: what an adapter emits while the model is talking.
//! Three invariants every adapter must hold: blocks carry a rising `index` and are opened
//! by `BlockStart`/closed by `BlockEnd`; `Usage` precedes `Finish`; `Finish` comes last.

use serde::{Deserialize, Serialize};

/// The kind of block a `BlockStart` opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Text,
    Reasoning,
    ToolUse,
}

/// Why the model stopped talking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model decided it was done.
    Stop,
    /// The model stopped to wait for tool results. The agent loop branches here.
    ToolCalls,
    /// Hit the `max_tokens` ceiling. The answer is cut off, not finished.
    Length,
    /// The server's content filter blocked it.
    ContentFilter,
    /// Cancelled by the user.
    Cancelled,
    /// The server reported an error but still closed the stream cleanly.
    Error,
}

/// Token statistics for one turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Taken from the server when it reports one, else summed; `Option` separates "server said 0" from "server said nothing".
    pub total_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.total_tokens
            .unwrap_or(self.input_tokens + self.output_tokens)
    }
}

/// One event on the stream; `ToolCallDelta.arguments` is a *fragment* of the JSON string, so joiners must concatenate raw text and never parse a fragment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamChunk {
    BlockStart {
        index: u32,
        kind: BlockKind,
    },
    TextDelta {
        index: u32,
        text: String,
    },
    ReasoningDelta {
        index: u32,
        text: String,
    },
    ToolCallDelta {
        index: u32,
        /// Only on the block's first delta. `None` afterwards.
        id: Option<String>,
        /// Only on the block's first delta. `None` afterwards.
        name: Option<String>,
        /// A fragment of the JSON string. May be empty.
        arguments: String,
    },
    /// Block closes; it carries no assembled content, or every adapter would duplicate the assembler's work.
    BlockEnd {
        index: u32,
    },
    Usage {
        usage: TokenUsage,
    },
    Finish {
        reason: FinishReason,
    },
}

impl StreamChunk {
    /// Answer text this chunk contributes; used by the UI token path, where only `TextDelta` is shown.
    pub fn answer_text(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn is_finish(&self) -> bool {
        matches!(self, Self::Finish { .. })
    }
}
