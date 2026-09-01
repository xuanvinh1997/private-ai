//! Hỏng hóc của tầng provider.
//!
//! Gộp bốn nguồn về một kiểu vì phía trên chỉ có một việc để làm với chúng: hiện một câu
//! tiếng Việt cho người dùng. Lỗi của tầng mô hình được giữ nguyên hình dạng ([`LlmError`]
//! có `code`) thay vì bị bóp thành chuỗi: chỗ gọi cần phân biệt "chưa cấu hình gì" với
//! "sai khoá", và **không được** phân biệt bằng cách so câu chữ.

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
