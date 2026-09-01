//! Hỏng hóc của tầng mô hình, phân loại bằng mã chứ không bằng câu chữ.
//!
//! Bản Python có một cây exception (`llm/__init__.py`): `ProviderUnavailable` ←
//! `NoProviderConfigured`, cộng `ProviderReadOnly` và `UnknownProvider`. Cây ấy chỉ dùng
//! được bằng `except`, nên mọi nơi muốn phân nhánh đều phải import đúng lớp. Ở đây gộp
//! thành **một kiểu, một trường `code`**: người gọi `match` trên `code`, và câu chữ
//! tiếng Việt chỉ để hiện cho người dùng.
//!
//! Luật: **không bao giờ route theo `message`.** Câu chữ được phép đổi bất cứ lúc nào.

use std::fmt;

/// Mã lỗi ổn định. Thêm biến thể là chuyện thường; đổi nghĩa một biến thể thì không.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LlmErrorCode {
    /// Không còn provider nào được cấu hình (`NoProviderConfigured`).
    NoProviderConfigured,
    /// Provider có đó nhưng không phục vụ được (`ProviderUnavailable`).
    ProviderUnavailable,
    /// Provider giữ mô hình ở nơi khác, nên vòng đời cục bộ không áp dụng
    /// (`ProviderReadOnly`).
    ProviderReadOnly,
    /// Không hàng nào mang id được hỏi (`UnknownProvider`).
    UnknownProvider,
    /// Máy chủ đòi khoá mà cấu hình không có.
    MissingCredential,
    /// 401/403.
    Auth,
    /// 429.
    RateLimit,
    /// Prompt vượt cửa sổ ngữ cảnh của mô hình.
    ContextWindowExceeded,
    /// Máy chủ trả thứ không đúng giao thức: JSON hỏng, thiếu trường bắt buộc.
    InvalidResponse,
    /// Hết thời gian chờ.
    Timeout,
    /// Người dùng huỷ.
    Cancelled,
    /// Adapter này không làm được việc đó (ví dụ nhúng trên một máy chủ chỉ có chat).
    Unsupported,
}

impl LlmErrorCode {
    /// Tên ngắn cho log. Không dùng để so sánh — so sánh bằng chính biến thể.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoProviderConfigured => "NO_PROVIDER_CONFIGURED",
            Self::ProviderUnavailable => "PROVIDER_UNAVAILABLE",
            Self::ProviderReadOnly => "PROVIDER_READ_ONLY",
            Self::UnknownProvider => "UNKNOWN_PROVIDER",
            Self::MissingCredential => "MISSING_CREDENTIAL",
            Self::Auth => "AUTH",
            Self::RateLimit => "RATE_LIMIT",
            Self::ContextWindowExceeded => "CONTEXT_WINDOW_EXCEEDED",
            Self::InvalidResponse => "INVALID_RESPONSE",
            Self::Timeout => "TIMEOUT",
            Self::Cancelled => "CANCELLED",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
}

/// Một hỏng hóc, kèm đủ dữ kiện để vừa phân nhánh vừa hiện ra màn hình.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmError {
    pub code: LlmErrorCode,
    pub message: String,
    /// Mã HTTP nếu hỏng hóc đến từ một phản hồi. `None` cho lỗi tầng vận chuyển.
    pub status: Option<u16>,
}

impl LlmError {
    pub fn new(code: LlmErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(LlmErrorCode::ProviderUnavailable, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(LlmErrorCode::InvalidResponse, message)
    }

    pub fn read_only(message: impl Into<String>) -> Self {
        Self::new(LlmErrorCode::ProviderReadOnly, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(LlmErrorCode::Unsupported, message)
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Đọc một phản hồi lỗi thành mã.
    ///
    /// Thân phản hồi *có* được ngó tới ở đúng một chỗ: cửa sổ ngữ cảnh bị tràn trả về
    /// 400 y hệt như một request sai cú pháp, và hai thứ đó cần hai cách xử lý khác hẳn
    /// nhau — một cái nén ngữ cảnh rồi thử lại, một cái là bug. Không máy chủ nào phân
    /// biệt chúng bằng mã, nên chỗ này buộc phải đoán.
    pub fn from_status(status: u16, body: &str) -> Self {
        let lowered = body.to_lowercase();
        let code = match status {
            401 | 403 => LlmErrorCode::Auth,
            404 => LlmErrorCode::ProviderUnavailable,
            408 => LlmErrorCode::Timeout,
            429 => LlmErrorCode::RateLimit,
            400 | 413 | 422
                if lowered.contains("context length")
                    || lowered.contains("context_length")
                    || lowered.contains("maximum context")
                    || lowered.contains("too long") =>
            {
                LlmErrorCode::ContextWindowExceeded
            }
            _ => LlmErrorCode::ProviderUnavailable,
        };
        let detail = body.trim();
        let message = if detail.is_empty() {
            format!("Máy chủ trả về HTTP {status}")
        } else {
            // Cắt: thân lỗi của vLLM có thể là cả một traceback.
            let mut shown: String = detail.chars().take(400).collect();
            if detail.chars().count() > 400 {
                shown.push('…');
            }
            format!("HTTP {status}: {shown}")
        };
        Self {
            code,
            message,
            status: Some(status),
        }
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for LlmError {}

impl From<reqwest::Error> for LlmError {
    fn from(err: reqwest::Error) -> Self {
        let code = if err.is_timeout() {
            LlmErrorCode::Timeout
        } else {
            LlmErrorCode::ProviderUnavailable
        };
        let status = err.status().map(|s| s.as_u16());
        Self {
            code,
            message: err.to_string(),
            status,
        }
    }
}

impl From<serde_json::Error> for LlmError {
    fn from(err: serde_json::Error) -> Self {
        Self::invalid(format!("JSON không đọc được: {err}"))
    }
}
