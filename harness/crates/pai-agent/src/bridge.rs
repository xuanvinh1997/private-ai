//! Translate between the journal's vocabulary and the model's.
//! Two `Message` types on purpose, so a change in `pai-llm` never makes old records
//! unreadable. Translation runs over the whole history, since a tool result needs its name.

use std::collections::HashMap;

use pai_llm::{ContentBlock as LlmBlock, Message as LlmMessage};
use pai_session::{ContentBlock as LogBlock, Message as LogMessage, Role};

/// Journal to model.
pub fn to_llm_history(history: &[LogMessage]) -> Vec<LlmMessage> {
    let mut names: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(history.len());

    for message in history {
        match message.role {
            Role::User => out.push(LlmMessage::User {
                content: blocks_to_llm(&message.content),
            }),
            Role::Assistant => {
                for block in &message.content {
                    if let LogBlock::ToolCall { id, name, .. } = block {
                        names.insert(id.clone(), name.clone());
                    }
                }
                out.push(LlmMessage::Assistant {
                    content: blocks_to_llm(&message.content),
                });
            }
            Role::Tool => {
                // One result per `tool` message: merging them loses the link from call to result.
                for block in &message.content {
                    if let LogBlock::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } = block
                    {
                        out.push(LlmMessage::Tool {
                            tool_call_id: call_id.clone(),
                            name: names.get(call_id).cloned().unwrap_or_default(),
                            content: content.clone(),
                            is_error: *is_error,
                        });
                    }
                }
            }
        }
    }
    out
}

fn blocks_to_llm(blocks: &[LogBlock]) -> Vec<LlmBlock> {
    blocks
        .iter()
        .filter_map(|block| match block {
            LogBlock::Text { text } => Some(LlmBlock::Text { text: text.clone() }),
            LogBlock::ToolCall {
                id,
                name,
                arguments,
            } => Some(LlmBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            // A tool result never sits inside an assistant or user message.
            LogBlock::ToolResult { .. } => None,
        })
        .collect()
}

/// Model to journal; assistant messages only, as the loop builds the other roles itself.
pub fn assistant_to_log(message: &LlmMessage) -> LogMessage {
    let LlmMessage::Assistant { content } = message else {
        return LogMessage {
            role: Role::Assistant,
            content: Vec::new(),
            source: None,
        };
    };
    LogMessage {
        role: Role::Assistant,
        content: content
            .iter()
            .filter_map(|block| match block {
                LlmBlock::Text { text } => Some(LogBlock::Text { text: text.clone() }),
                LlmBlock::ToolUse {
                    id,
                    name,
                    arguments,
                } => Some(LogBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                // Reasoning is deliberately not journalled as content: it is never resent, so keeping it misstates history.
                _ => None,
            })
            .collect(),
        source: None,
    }
}
