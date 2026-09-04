//! Provider-layer failures: four sources in one type, since callers only ever show one sentence to the
//! user. [`LlmError`] keeps its shape and `code`, so "not configured" and "bad key" are told apart by
//! code rather than by comparing message text.

use pai_llm::LlmError;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Llm(#[from] LlmError),
    #[error("không có nhà cung cấp nào mang id `{0}`")]
    NotFound(String),
    #[error("cấu hình không hợp lệ: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ProviderError>;
