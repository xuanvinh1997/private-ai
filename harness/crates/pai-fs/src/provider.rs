//! Seam hệ tệp.
//!
//! Tool không gọi `std::fs` mà gọi qua đây. Lý do không phải là trừu tượng cho vui: khi
//! provider này trỏ vào một sandbox hoặc một máy từ xa, cả năm tool đi theo mà không tool
//! nào phải sửa. Đó là điều duy nhất khiến "đổi một provider là đổi cả sản phẩm" thành
//! sự thật thay vì khẩu hiệu.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use pai_core::ServiceKey;

use crate::path::looks_binary;

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("{0}")]
    Io(String),
    #[error("{0} trông như tệp nhị phân; hãy mở nó bằng công cụ khác")]
    Binary(PathBuf),
    #[error("{0} không phải một tệp")]
    NotAFile(PathBuf),
}

impl From<std::io::Error> for FsError {
    fn from(err: std::io::Error) -> FsError {
        FsError::Io(err.to_string())
    }
}

#[async_trait]
pub trait FsProvider: Send + Sync + 'static {
    async fn read_text(&self, path: &Path) -> Result<String, FsError>;
    async fn write_text(&self, path: &Path, content: &str) -> Result<(), FsError>;
    async fn exists(&self, path: &Path) -> bool;
}

pub enum Fs {}
impl ServiceKey for Fs {
    type Api = dyn FsProvider;
    const NAME: &'static str = "fs";
}

/// Đĩa của chính máy này.
#[derive(Default)]
pub struct LocalFs;

#[async_trait]
impl FsProvider for LocalFs {
    async fn read_text(&self, path: &Path) -> Result<String, FsError> {
        let metadata = tokio::fs::metadata(path).await?;
        if !metadata.is_file() {
            return Err(FsError::NotAFile(path.to_path_buf()));
        }
        let bytes = tokio::fs::read(path).await?;
        if looks_binary(&bytes) {
            return Err(FsError::Binary(path.to_path_buf()));
        }
        // Tới đây đã biết không có byte không; phần còn lại vẫn có thể là UTF-8 hỏng, và
        // lúc đó thay ký tự là đúng: tệp *là* văn bản, chỉ có vài byte lẻ.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn write_text(&self, path: &Path, content: &str) -> Result<(), FsError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    async fn exists(&self, path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }
}
