//! Document library errors.
//! Each variant exists because the user must be able to read it and know what to do next;
//! per-file read errors keep enough context to be shown directly in the UI.

/// Errors at the document library layer.
#[derive(Debug, thiserror::Error)]
pub enum RagError {
    /// Service is down, silent, or returned something unreadable; the message says what to do.
    #[error("{0}")]
    Service(String),

    /// The bytes or format of one input file are invalid. A folder sync may fingerprint
    /// this error because retrying unchanged bytes cannot produce a different result.
    #[error("{0}")]
    Extraction(String),

    #[error("không có tài liệu nào mang mã `{0}`")]
    NotFound(String),

    /// A real capability that is currently off: no embedding model chosen, no project open.
    #[error("{0}")]
    Unavailable(String),
}

impl RagError {
    pub(crate) fn is_extraction(&self) -> bool {
        matches!(self, Self::Extraction(_))
    }
}
