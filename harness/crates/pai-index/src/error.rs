//! Index errors.
//! Deliberately few variants: the index is a cache rebuildable from source, so nearly
//! everything here means report and rebuild. A single file's failure is logged, not raised.

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("kho chỉ mục: {0}")]
    Store(String),
    #[error("không quét được {0}: {1}")]
    Scan(String, String),
    #[error("{0}")]
    Unavailable(String),
}

impl From<rusqlite::Error> for IndexError {
    fn from(err: rusqlite::Error) -> IndexError {
        IndexError::Store(err.to_string())
    }
}
