//! Từ vựng của một lần gọi: yêu cầu, kết quả, và cái trait mà tool nào cũng cài.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::name::ToolName;
use crate::schema::{ToolMeta, ToolSchema};

/// Lỗi mà **thân tool** được phép trả về.
///
/// Chỉ tồn tại bên trong đường ống. Ở biên ngoài cùng nó luôn bị gấp thành một
/// [`ToolOutcome`] có `is_error`, vì một `Result` lọt ra ngoài sẽ kết thúc lượt trong im
/// lặng và mô hình không đọc được gì.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tham số không dùng được. Mô hình sửa được, nên nói cho nó biết sửa gì.
    #[error("tham số không hợp lệ: {0}")]
    Invalid(String),
    /// Thân tool chạy nhưng hỏng.
    #[error("{0}")]
    Failed(String),
    /// Người dùng được hỏi và đã nói không.
    #[error("người dùng từ chối: {0}")]
    Refused(String),
}

/// Kết quả một lần gọi, sau khi mọi tầng đã xong.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolOutcome {
    /// Văn bản mô hình đọc.
    pub content: String,
    /// Giá trị trả về đã có kiểu của tool, nếu có. Đây mới là thứ giao diện nên vẽ.
    pub structured: Option<Value>,
    pub is_error: bool,
    /// Metadata cho host — diff, locator của phần tràn, lý do từ chối. Không đi ra mô hình.
    pub meta: Map<String, Value>,
}

impl ToolOutcome {
    pub fn ok(content: impl Into<String>) -> ToolOutcome {
        ToolOutcome {
            content: content.into(),
            structured: None,
            is_error: false,
            meta: Map::new(),
        }
    }

    /// Một thất bại vẫn là một kết quả đọc được, không phải một ngoại lệ.
    pub fn error(content: impl Into<String>) -> ToolOutcome {
        ToolOutcome {
            is_error: true,
            ..ToolOutcome::ok(content)
        }
    }

    pub fn with_structured(mut self, value: Value) -> ToolOutcome {
        self.structured = Some(value);
        self
    }

    pub fn with_meta(mut self, key: impl Into<String>, value: Value) -> ToolOutcome {
        self.meta.insert(key.into(), value);
        self
    }
}

/// Hỏi người dùng một giá trị khớp một JSON Schema.
///
/// Khác [`crate::pipeline::Approver`] ở chỗ nó xin **dữ liệu**, không xin phép: "thư mục
/// nào?" chứ không phải "cho chạy không?". Tách ra vì một giao diện có thể muốn trả lời
/// hai câu hỏi đó bằng hai loại hộp thoại khác nhau, và vì gộp lại sẽ cám dỗ người viết
/// dùng một lần elicit như một lần phê duyệt.
#[async_trait]
pub trait Elicitor: Send + Sync + 'static {
    /// `None` nghĩa là không lấy được câu trả lời — huỷ, hết giờ, hoặc không có giao diện.
    async fn elicit(&self, prompt: &str, schema: &Value) -> Option<Value>;
}

/// Một lần gọi đang chạy.
///
/// Tham số ở đây đã đi qua ghim: cái tool đọc được là cái host quyết định, không phải cái
/// mô hình gửi.
pub struct Invocation {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    elicitor: Option<Arc<dyn Elicitor>>,
    /// Bị huỷ khi hết giờ. Thân tool nên theo dõi nó để bỏ việc dở thay vì chạy tiếp
    /// trong nền sau khi kết quả đã bị vứt.
    cancel: CancellationToken,
}

impl Invocation {
    pub fn new(
        name: ToolName,
        call_id: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> Invocation {
        Invocation {
            name,
            call_id: call_id.into(),
            arguments,
            elicitor: None,
            cancel: CancellationToken::new(),
        }
    }

    pub fn with_elicitor(mut self, elicitor: Option<Arc<dyn Elicitor>>) -> Invocation {
        self.elicitor = elicitor;
        self
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn arg(&self, field: &str) -> Option<&Value> {
        self.arguments.get(field)
    }

    pub fn str_arg(&self, field: &str) -> Option<&str> {
        self.arguments.get(field).and_then(Value::as_str)
    }

    /// Hỏi người dùng một giá trị. Không có giao diện nào cắm vào thì trả `None` —
    /// fail-closed, giống hệt phê duyệt.
    pub async fn elicit(&self, prompt: &str, schema: &Value) -> Option<Value> {
        let elicitor = self.elicitor.clone()?;
        elicitor.elicit(prompt, schema).await
    }

    /// Ảnh chụp tham số để ghi sổ và để hiện lên giao diện.
    pub fn snapshot(&self) -> Value {
        json!({ "name": self.name.as_str(), "call_id": self.call_id, "arguments": self.arguments })
    }
}

/// Một tool.
///
/// Object-safe: sổ đăng ký giữ `Arc<dyn Tool>`, và một tool đến từ MCP không phải một
/// kiểu tĩnh nào cả.
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// Cái mô hình thấy. Sổ đăng ký còn sửa mô tả và giấu tham số ghim trước khi gửi đi.
    fn schema(&self) -> ToolSchema;

    /// Cái chỉ host thấy. Mặc định là giả định xấu nhất — xem [`ToolMeta::default`].
    fn meta(&self) -> ToolMeta {
        ToolMeta::default()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError>;

    /// Bất biến cuối cùng, đồng bộ, **chỉ đụng `content`**.
    ///
    /// Đồng bộ để nó không thể đi hỏi ai thêm; chỉ đụng content để nó không thể lật
    /// `is_error` sau khi mọi tầng chính sách đã chạy xong và không còn ai kiểm lại.
    fn finalize(&self, _content: &mut String) {}
}
