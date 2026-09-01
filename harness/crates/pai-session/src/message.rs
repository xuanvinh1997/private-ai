//! Từ vựng message tối thiểu.
//!
//! Về lâu dài `pai-llm` sở hữu từ vựng này. Sổ vẫn phải khai báo nó ở đây vì phép chiếu
//! cần đọc được đúng **một** điều: message có rỗng nội dung không. Đó là khác biệt giữa
//! một bước bị cụt vì hết token — chỉ còn `usage` để ghi lại — và một message thật.
//!
//! Ngoài chỗ đó, sổ không diễn giải nội dung. Đóng khung (`<context>…`) là việc của bên
//! sản xuất, không phải của phép chiếu: nếu phép chiếu thêm chữ, bản ghi sẽ không còn
//! dựng lại đúng thứ mô hình đã thấy.

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
        /// Chuỗi JSON **chưa parse**. Mô hình sinh ra chuỗi này; parse rồi in lại sẽ đổi
        /// byte, mà byte chính là thứ phải phát lại được.
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
    /// Ai đẻ ra message này, khi nó không đến từ người dùng: `subagent-report`,
    /// `compaction-checkpoint`… Mô hình không thấy trường này; giao diện thì cần.
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
