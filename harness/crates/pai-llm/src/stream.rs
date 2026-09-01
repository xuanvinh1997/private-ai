//! Từ vựng luồng: cái mà một adapter phát ra trong lúc mô hình đang nói.
//!
//! Đây là chỗ bản Python không có gì để port. `AIMessageChunk` của LangChain được gấp
//! lại bằng toán tử `+`, nên hình dạng thật của một chunk — và toàn bộ wire format của
//! tool-calling — nằm ẩn trong thư viện. Bản Rust phải tự khai, nên khai cho đúng.
//!
//! Hình dạng mượn từ dsh (`packages/llm/llm/src/types.ts`), với **ba bất biến** mà mọi
//! adapter phải giữ và [`crate::assembler::BlockAssembler`] dựa vào:
//!
//! 1. Mỗi khối nội dung có một `index` tăng dần, mở bằng `BlockStart` và đóng bằng
//!    `BlockEnd`. Delta của hai khối khác nhau **được phép** xen kẽ.
//! 2. `Usage` (nếu có) đứng **trước** `Finish`.
//! 3. `Finish` là chunk cuối cùng. Sau nó không còn gì. Một luồng kết thúc bằng đúng một
//!    `Finish`, hoặc bằng một `Err` — không có khả năng thứ ba.

use serde::{Deserialize, Serialize};

/// Loại khối mà một `BlockStart` mở ra.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Text,
    Reasoning,
    ToolUse,
}

/// Vì sao mô hình ngừng nói.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Mô hình tự thấy đã xong.
    Stop,
    /// Mô hình dừng để đợi kết quả tool. Vòng lặp agent phân nhánh ở đây.
    ToolCalls,
    /// Chạm trần `max_tokens`. Câu trả lời **bị cắt giữa chừng**, không phải xong.
    Length,
    /// Bộ lọc nội dung của máy chủ chặn lại.
    ContentFilter,
    /// Người dùng huỷ.
    Cancelled,
    /// Máy chủ báo dừng vì lỗi, nhưng vẫn đóng luồng tử tế.
    Error,
}

/// Thống kê token của một lượt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Máy chủ nào báo thì lấy; không thì cộng lại. Giữ `Option` để phân biệt "máy chủ
    /// nói 0" với "máy chủ không nói".
    pub total_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.total_tokens
            .unwrap_or(self.input_tokens + self.output_tokens)
    }
}

/// Một sự kiện trên luồng.
///
/// `ToolCallDelta.arguments` là **một mảnh** của chuỗi JSON, không phải cả chuỗi. OpenAI
/// gửi tên tool ở delta đầu rồi nhỏ giọt tham số qua hàng chục event, và điểm cắt rơi
/// vào giữa một escape `\"` là chuyện bình thường. Ai ghép chúng lại phải nối chuỗi
/// thuần, tuyệt đối không parse từng mảnh.
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
        /// Chỉ có ở delta đầu của khối. `None` ở các delta sau.
        id: Option<String>,
        /// Chỉ có ở delta đầu của khối. `None` ở các delta sau.
        name: Option<String>,
        /// Mảnh chuỗi JSON. Có thể rỗng.
        arguments: String,
    },
    /// Khối đóng lại. **Không mang theo nội dung đã ráp**: nếu mang thì mọi adapter đều
    /// phải giữ một bản sao của cả khối, tức là làm đúng công việc của bộ ráp, hai lần.
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
    /// Văn bản trả lời mà chunk này đóng góp. Dùng cho đường token ra giao diện, nơi chỉ
    /// `TextDelta` được hiện — reasoning đi kênh khác, tool call đi kênh khác nữa.
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
