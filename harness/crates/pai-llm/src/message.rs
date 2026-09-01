//! Từ vựng hội thoại: thứ đi *vào* một request và thứ được ráp lại *từ* một luồng.
//!
//! Bản Python không có từ vựng riêng — nó mượn thẳng `BaseChatModel` của LangChain, nên
//! hình dạng message và wire format tool-calling nằm hết trong thư viện. Ở đây phải tự
//! khai, và một khi đã tự khai thì chọn được hình dạng đúng: **provider-neutral**, mỗi
//! adapter tự dịch sang giao thức của mình. Đó là luật phụ thuộc của harness — plugin mở
//! rộng phụ thuộc vào định nghĩa seam, không bao giờ vào một provider cụ thể.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::LlmError;

/// Định danh một lời gọi tool. OpenAI phát ra chuỗi riêng của nó; Ollama không phát gì
/// nên adapter tự sinh. Kiểu là `String` để cả hai đều nhét vừa mà không phải mã hoá lại.
pub type ToolCallId = String;

/// Một mảnh nội dung trong một message.
///
/// `ToolUse.arguments` **là chuỗi JSON thô, không phải `Value`**. Đây là quyết định
/// trung tâm: tham số tool đến từng mảnh qua nhiều chunk, nên trước lúc luồng đóng thì
/// nó *chưa* phải JSON hợp lệ. Nếu kiểu ở đây là `Value` thì bộ ráp buộc phải parse lại
/// sau mỗi mảnh — vừa phí vừa sai. Giữ nguyên chuỗi model phát ra, ai cần thì parse một
/// lần, và cái model nói ra vẫn ghi lại được nguyên văn khi nó phát JSON hỏng.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// Suy luận hiện ra (Ollama gọi là `thinking`, một số máy chủ OpenAI-compatible gọi
    /// là `reasoning_content`). Tách khỏi `Text` vì giao diện hiện nó khác, và vì nó
    /// **không** được gửi ngược lại cho vòng sau.
    Reasoning {
        text: String,
    },
    ToolUse {
        id: ToolCallId,
        name: String,
        /// Chuỗi JSON thô. Xem ghi chú của enum.
        arguments: String,
    },
    /// Kết quả một tool, ở dạng mang đi được.
    ///
    /// Trên dây thì kết quả tool là *một message có vai riêng* (`Message::Tool`), nhưng
    /// trong sổ tay phiên thì để lời gọi và kết quả cạnh nhau tiện hơn nhiều. Hai hình
    /// dạng, một sự thật: [`Message::from_tool_result`] đổi chiều này sang chiều kia.
    ToolResult {
        tool_call_id: ToolCallId,
        content: String,
        is_error: bool,
    },
    /// Ảnh đã mã hoá base64. Giữ base64 chứ không giữ đường dẫn, vì hai adapter cần hai
    /// khuôn khác nhau (Ollama: mảng `images`; OpenAI: `data:` URL) và cả hai đều dựng
    /// được từ base64 mà không phải đụng đĩa lần nữa.
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

    /// Phần văn bản của khối, nếu có. `Reasoning` **không** tính: nó không phải câu trả lời.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// Một lời gọi tool đã ráp xong, tách khỏi `ContentBlock` để người gọi khỏi phải `match`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    /// Chuỗi JSON thô đúng như model phát ra.
    pub arguments: String,
}

impl ToolCall {
    /// Parse tham số. Đây là **chỗ duy nhất** được phép parse, và nó trả `Result` vì một
    /// model nhỏ hoàn toàn có thể phát ra JSON hỏng — đó là chuyện thường ngày, không
    /// phải bug của ta, và vòng lặp agent phải nói lại lỗi cho model bằng văn bản.
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

/// Một lượt trong hội thoại.
///
/// `System` giữ `String` chứ không `Vec<ContentBlock>`: chưa provider nào nhận ảnh trong
/// vai system, nên cho phép cấu trúc ấy chỉ là mời gọi một nhánh không bao giờ chạy.
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
        /// Tên tool. Ollama cần nó trên dây (`tool_name`); OpenAI thì không.
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

    /// Đổi khối `ToolResult` trong sổ tay thành message vai `tool` trên dây.
    /// `name` phải lấy từ lời gọi tương ứng — khối kết quả không mang nó.
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

    /// Toàn bộ văn bản trả lời của message, nối liền. Bỏ qua reasoning và tool call.
    pub fn text(&self) -> String {
        match self {
            Self::System { content } => content.clone(),
            Self::Tool { content, .. } => content.clone(),
            Self::User { content } | Self::Assistant { content } => {
                content.iter().filter_map(ContentBlock::as_text).collect()
            }
        }
    }

    /// Mọi lời gọi tool trong message, theo thứ tự.
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

/// Mô tả một tool cho mô hình.
///
/// `parameters` là một JSON Schema thô. Không có kiểu chặt hơn ở đây là cố ý: schema đến
/// từ MCP dưới dạng JSON, và dựng lại nó thành kiểu Rust chỉ để rồi tuần tự hoá ngược ra
/// JSON là mất mát ròng — mọi từ khoá lạ mà một máy chủ MCP dùng sẽ bị bào mất.
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

/// Một request tới mô hình.
///
/// Cố tình *không* mang `provider`: adapter đã biết mình nói chuyện với ai. Cũng không
/// mang token huỷ — huỷ trong Rust là **thả cái stream đi**, và cái đó không cần trường.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Vec<String>,
    /// Ollama-only: `"5m"` để giữ mô hình trong VRAM, `"0"` để nhả ngay. `None` = theo
    /// mặc định của máy chủ. Adapter OpenAI bỏ qua trường này thay vì báo lỗi: nó là
    /// gợi ý về vòng đời, không phải một yêu cầu về nội dung.
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
