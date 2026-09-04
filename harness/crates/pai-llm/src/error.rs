//! Model-layer failures, classified by code rather than by wording.
//! One type with a `code` field: callers `match` on the code, and the Vietnamese text is
//! only for display. Rule: never route on `message`, the wording may change at any time.

use std::fmt;

/// Stable error code. Adding a variant is routine; changing what one means is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LlmErrorCode {
    /// No provider is configured any more (`NoProviderConfigured`).
    NoProviderConfigured,
    /// The provider exists but cannot serve (`ProviderUnavailable`).
    ProviderUnavailable,
    /// The provider keeps models elsewhere, so local lifecycle does not apply (`ProviderReadOnly`).
    ProviderReadOnly,
    /// No row carries the requested id (`UnknownProvider`).
    UnknownProvider,
    /// The server wants a key the config does not have.
    MissingCredential,
    /// 401/403.
    Auth,
    /// 429.
    RateLimit,
    /// The prompt exceeds the model's context window.
    ContextWindowExceeded,
    /// The server returned something off-protocol: broken JSON, a missing required field.
    InvalidResponse,
    /// Timed out.
    Timeout,
    /// Cancelled by the user.
    Cancelled,
    /// This adapter cannot do that (for example embedding on a chat-only server).
    Unsupported,
}

impl LlmErrorCode {
    /// Short name for logs. Not for comparison - compare the variant itself.
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

/// One failure, carrying enough to both branch on and show on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlmError {
    pub code: LlmErrorCode,
    pub message: String,
    /// HTTP status when the failure came from a response. `None` for transport errors.
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

    /// Read an error response into a code; the body is inspected in exactly one place, because a blown context window returns the same 400 as a malformed request.
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
            // Truncate: a vLLM error body can be a whole traceback.
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
