//! Lỗi của chỉ mục.
//!
//! Cố tình ít nhánh: chỉ mục là một **bộ nhớ đệm dựng lại được từ mã nguồn**, nên gần như
//! mọi sự cố ở đây có một cách xử lý đúng duy nhất — nói ra, rồi dựng lại. Chỉ những thứ
//! không tự sửa được mới lên tới đây; lỗi của **một tệp** thì không, nó được ghi log và
//! lần quét đi tiếp.

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
