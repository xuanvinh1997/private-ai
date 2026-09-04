//! Document library errors.
//! Each variant exists because the user must be able to read it and know what to do next;
//! per-file read errors now come from `services/rag/` as ready-made messages.

/// Errors at the document library layer.
#[derive(Debug, thiserror::Error)]
pub enum RagError {
    /// Service is down, silent, or returned something unreadable; the message says what to do.
    #[error("{0}")]
    Service(String),

    #[error("không có tài liệu nào mang mã `{0}`")]
    NotFound(String),

    /// A real capability that is currently off: no embedding model chosen, no project open.
    #[error("{0}")]
    Unavailable(String),
}

