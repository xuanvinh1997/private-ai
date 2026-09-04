//! Folds a [`StreamChunk`] stream into a finished [`Message`].
//! One rule carries the module: tool arguments are strings, and they are only
//! concatenated, never parsed until the stream closes. Off-spec chunks are logged, not fatal.

use std::collections::BTreeMap;

use tracing::warn;

use crate::message::{ContentBlock, Message, ToolCall};
use crate::stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};

/// A block still being assembled.
#[derive(Clone, Debug)]
enum Partial {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: Option<String>,
        name: String,
        arguments: String,
    },
}

impl Partial {
    fn for_kind(kind: BlockKind) -> Self {
        match kind {
            BlockKind::Text => Self::Text(String::new()),
            BlockKind::Reasoning => Self::Reasoning(String::new()),
            BlockKind::ToolUse => Self::ToolCall {
                id: None,
                name: String::new(),
                arguments: String::new(),
            },
        }
    }

    fn kind(&self) -> BlockKind {
        match self {
            Self::Text(_) => BlockKind::Text,
            Self::Reasoning(_) => BlockKind::Reasoning,
            Self::ToolCall { .. } => BlockKind::ToolUse,
        }
    }
}

/// Collects chunks into blocks, then into a message; blocks live in a `BTreeMap` because the server picks the `index` order and OpenAI numbers tool calls separately.
#[derive(Debug, Default)]
pub struct BlockAssembler {
    blocks: BTreeMap<u32, Partial>,
    usage: Option<TokenUsage>,
    finish: Option<FinishReason>,
}

impl BlockAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Swallow one chunk; no `Result`, because no chunk is fatal here - real stream errors arrive as `Err` from the stream itself.
    pub fn push(&mut self, chunk: &StreamChunk) {
        if self.finish.is_some() {
            // Invariant: nothing follows `Finish`. Accepting more would build a message nobody can reconstruct.
            warn!(?chunk, "chunk arrived after Finish, ignoring");
            return;
        }
        match chunk {
            StreamChunk::BlockStart { index, kind } => {
                self.blocks
                    .entry(*index)
                    .or_insert_with(|| Partial::for_kind(*kind));
            }
            StreamChunk::TextDelta { index, text } => {
                if let Some(Partial::Text(buffer)) = self.slot(*index, BlockKind::Text) {
                    buffer.push_str(text);
                }
            }
            StreamChunk::ReasoningDelta { index, text } => {
                if let Some(Partial::Reasoning(buffer)) = self.slot(*index, BlockKind::Reasoning) {
                    buffer.push_str(text);
                }
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                if let Some(Partial::ToolCall {
                    id: slot_id,
                    name: slot_name,
                    arguments: buffer,
                }) = self.slot(*index, BlockKind::ToolUse)
                {
                    // Id and name only arrive on the first delta; keep what we have so a repeat cannot duplicate it.
                    if slot_id.is_none()
                        && let Some(value) = id
                    {
                        *slot_id = Some(value.clone());
                    }
                    if slot_name.is_empty()
                        && let Some(value) = name
                    {
                        value.clone_into(slot_name);
                    }
                    // The most important line in the module: plain concatenation, no parsing.
                    buffer.push_str(arguments);
                }
            }
            StreamChunk::BlockEnd { .. } => {
                // Nothing to do; `BlockEnd` stays on the protocol so the UI knows when to stop the caret.
            }
            StreamChunk::Usage { usage } => self.usage = Some(*usage),
            StreamChunk::Finish { reason } => self.finish = Some(*reason),
        }
    }

    /// The slot for a block, opened if absent; `None` when `index` already belongs to another block kind, in which case dropping the new fragment loses less than overwriting.
    fn slot(&mut self, index: u32, kind: BlockKind) -> Option<&mut Partial> {
        let slot = self
            .blocks
            .entry(index)
            .or_insert_with(|| Partial::for_kind(kind));
        if slot.kind() != kind {
            warn!(index, ?kind, actual = ?slot.kind(), "block changed kind mid-stream, dropping delta");
            return None;
        }
        Some(slot)
    }

    /// Has `Finish` been seen?
    pub fn is_finished(&self) -> bool {
        self.finish.is_some()
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish
    }

    pub fn usage(&self) -> Option<TokenUsage> {
        self.usage
    }

    /// The assembled blocks in `index` order; empty text blocks and unnamed tool calls are dropped, since neither carries anything to act on.
    pub fn blocks(&self) -> Vec<ContentBlock> {
        self.blocks
            .iter()
            .filter_map(|(index, partial)| match partial {
                Partial::Text(text) if text.is_empty() => None,
                Partial::Text(text) => Some(ContentBlock::Text { text: text.clone() }),
                Partial::Reasoning(text) if text.is_empty() => None,
                Partial::Reasoning(text) => Some(ContentBlock::Reasoning { text: text.clone() }),
                Partial::ToolCall { name, .. } if name.is_empty() => None,
                Partial::ToolCall {
                    id,
                    name,
                    arguments,
                } => Some(ContentBlock::ToolUse {
                    // Ollama emits no id. Derive one from `index` so it is stable within a turn and matches the `tool` role message next round.
                    id: id.clone().unwrap_or_else(|| format!("call_{index}")),
                    name: name.clone(),
                    arguments: normalize_arguments(arguments),
                }),
            })
            .collect()
    }

    /// The assistant message, assembled from every block received.
    pub fn message(&self) -> Message {
        Message::Assistant {
            content: self.blocks(),
        }
    }

    pub fn into_message(self) -> Message {
        self.message()
    }

    /// Every finished tool call. The agent loop branches on this.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.message().tool_calls()
    }

    /// Answer text, concatenated. Excludes reasoning.
    pub fn text(&self) -> String {
        self.message().text()
    }

    /// Clear for reuse next round. Cheaper than building a new one across many agent loops.
    pub fn reset(&mut self) {
        self.blocks.clear();
        self.usage = None;
        self.finish = None;
    }
}

/// Empty arguments become an empty object: OpenAI sends `"arguments": ""` for a no-parameter tool, and `serde_json` reads nothing from an empty string.
fn normalize_arguments(raw: &str) -> String {
    if raw.trim().is_empty() {
        "{}".to_string()
    } else {
        raw.to_string()
    }
}
