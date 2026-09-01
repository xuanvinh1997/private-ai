//! Gấp một luồng [`StreamChunk`] thành một [`Message`] hoàn chỉnh.
//!
//! Thay cho phép `+` trên `AIMessageChunk` của LangChain (`agent/graph.py:347-350`).
//! Toàn bộ giá trị của mô-đun này nằm ở một câu: **tham số tool là chuỗi, và chuỗi chỉ
//! được nối, không được parse cho tới khi luồng đóng.** Mọi bug tool-calling mà người ta
//! gặp khi tự viết adapter đều là biến thể của việc vi phạm câu đó.
//!
//! Bộ ráp cố tình **khoan dung**: một máy chủ phát ra delta cho khối chưa mở, hay phát
//! thêm chunk sau `Finish`, thì bị ghi log chứ không làm hỏng lượt. Nghiêm khắc ở đây
//! nghĩa là một máy chủ OpenAI-compatible hơi lệch chuẩn sẽ giết cả câu trả lời.

use std::collections::BTreeMap;

use tracing::warn;

use crate::message::{ContentBlock, Message, ToolCall};
use crate::stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};

/// Một khối đang được ráp dở.
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

/// Gom chunk lại thành khối, rồi thành message.
///
/// Khối được giữ trong `BTreeMap` chứ không `Vec`: thứ tự xuất hiện của `index` do máy
/// chủ quyết định, và OpenAI đánh số tool call theo một dãy riêng, tách khỏi khối văn
/// bản. `BTreeMap` cho thứ tự theo `index` mà không cần adapter hứa hẹn gì thêm.
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

    /// Nuốt một chunk.
    ///
    /// Không trả `Result`: không có chunk nào là lỗi chí mạng của bộ ráp. Lỗi thật của
    /// luồng đến dưới dạng `Err` từ chính cái stream, và người gọi thấy nó trước.
    pub fn push(&mut self, chunk: &StreamChunk) {
        if self.finish.is_some() {
            // Bất biến: không gì đứng sau `Finish`. Máy chủ phá luật thì ta ghi log và bỏ
            // qua — nhận thêm vào sẽ cho ra một message mà không ai tái dựng lại được.
            warn!(?chunk, "chunk đến sau Finish, bỏ qua");
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
                    // Id và tên chỉ đến ở delta đầu; delta sau gửi `None`. Đã có rồi thì
                    // giữ nguyên — một máy chủ lặp lại id không được phép làm nó nhân đôi.
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
                    // Đây là dòng quan trọng nhất của cả mô-đun: nối thuần, không parse.
                    buffer.push_str(arguments);
                }
            }
            StreamChunk::BlockEnd { .. } => {
                // Không phải làm gì: khối đã nằm trong bản đồ và không nhận thêm delta
                // nào nữa vì adapter không phát nữa. Giữ `BlockEnd` trên giao thức là để
                // giao diện biết lúc nào đóng con trỏ nhấp nháy.
            }
            StreamChunk::Usage { usage } => self.usage = Some(*usage),
            StreamChunk::Finish { reason } => self.finish = Some(*reason),
        }
    }

    /// Ô của một khối, mở mới nếu chưa có.
    ///
    /// Trả `None` khi `index` đã thuộc về một loại khối khác. Chuyện đó nghĩa là máy chủ
    /// dùng lại một số thứ tự cho hai mục đích; ghi đè khối cũ sẽ làm mất văn bản đã
    /// nhận, nên bỏ mảnh mới đi là hư hại nhỏ hơn.
    fn slot(&mut self, index: u32, kind: BlockKind) -> Option<&mut Partial> {
        let slot = self
            .blocks
            .entry(index)
            .or_insert_with(|| Partial::for_kind(kind));
        if slot.kind() != kind {
            warn!(index, ?kind, actual = ?slot.kind(), "khối đổi loại giữa chừng, bỏ qua delta");
            return None;
        }
        Some(slot)
    }

    /// Đã thấy `Finish` chưa.
    pub fn is_finished(&self) -> bool {
        self.finish.is_some()
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish
    }

    pub fn usage(&self) -> Option<TokenUsage> {
        self.usage
    }

    /// Các khối đã ráp, theo thứ tự `index`.
    ///
    /// Khối văn bản rỗng bị loại: một máy chủ mở khối rồi không gửi gì là chuyện thường
    /// (Ollama phát một message rỗng ở dòng `done`), và một `Text { text: "" }` trong sổ
    /// tay chỉ là rác. Lời gọi tool **không tên** cũng bị loại — không có tên thì không
    /// có gì để gọi.
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
                    // Ollama không phát id. Sinh từ `index` để nó ổn định trong một lượt
                    // và khớp được với message vai `tool` gửi ở vòng sau.
                    id: id.clone().unwrap_or_else(|| format!("call_{index}")),
                    name: name.clone(),
                    arguments: normalize_arguments(arguments),
                }),
            })
            .collect()
    }

    /// Message của trợ lý, ráp từ mọi khối đã nhận.
    pub fn message(&self) -> Message {
        Message::Assistant {
            content: self.blocks(),
        }
    }

    pub fn into_message(self) -> Message {
        self.message()
    }

    /// Mọi lời gọi tool đã ráp xong. Vòng lặp agent phân nhánh trên cái này.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.message().tool_calls()
    }

    /// Văn bản trả lời, nối liền. Không gồm reasoning.
    pub fn text(&self) -> String {
        self.message().text()
    }

    /// Dọn để dùng lại cho vòng sau. Rẻ hơn dựng mới khi vòng lặp agent chạy nhiều vòng.
    pub fn reset(&mut self) {
        self.blocks.clear();
        self.usage = None;
        self.finish = None;
    }
}

/// Tham số rỗng thành object rỗng.
///
/// OpenAI gửi `"arguments": ""` cho tool không tham số, và `serde_json` không đọc chuỗi
/// rỗng thành gì cả. Chuẩn hoá ở đây chứ không ở chỗ parse, để cái nằm trong sổ tay đã
/// là JSON hợp lệ ngay từ đầu.
fn normalize_arguments(raw: &str) -> String {
    if raw.trim().is_empty() {
        "{}".to_string()
    } else {
        raw.to_string()
    }
}
