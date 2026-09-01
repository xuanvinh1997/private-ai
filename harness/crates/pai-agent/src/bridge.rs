//! Dịch giữa từ vựng của sổ và từ vựng của mô hình.
//!
//! Hai crate cố ý có hai kiểu `Message` riêng. Sổ chỉ cần biết **một** điều về nội dung —
//! nó có rỗng không — và không diễn giải gì thêm; nếu sổ hiểu từ vựng stream thì mỗi lần
//! `pai-llm` đổi hình dạng là một lần bản ghi cũ đọc không ra.
//!
//! Phép dịch chạy trên **cả lịch sử**, không trên từng message. Lý do rất cụ thể: trên
//! dây, một kết quả tool phải mang theo tên tool, nhưng trong sổ thì tên chỉ nằm ở lời
//! gọi tương ứng. Nhìn cả dãy thì tra được; nhìn một message thì phải bịa.

use std::collections::HashMap;

use pai_llm::{ContentBlock as LlmBlock, Message as LlmMessage};
use pai_session::{ContentBlock as LogBlock, Message as LogMessage, Role};

/// Sổ → mô hình.
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
                // Một message vai `tool` mang đúng một kết quả: gộp nhiều kết quả vào một
                // message là mất mối nối giữa lời gọi và kết quả của nó.
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
            // Kết quả tool không đứng trong message của trợ lý hay của người dùng.
            LogBlock::ToolResult { .. } => None,
        })
        .collect()
}

/// Mô hình → sổ. Chỉ dùng cho message trợ lý; những vai khác do vòng lặp tự dựng.
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
                // Suy luận cố ý không vào sổ dưới dạng nội dung: nó không được gửi lại
                // cho vòng sau, nên để nó trong lịch sử là dựng lại sai thứ mô hình thấy.
                _ => None,
            })
            .collect(),
        source: None,
    }
}
