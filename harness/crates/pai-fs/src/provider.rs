//! The filesystem seam.
//!
//! Tools do not call `std::fs`; they go through here. The reason is not abstraction for its
//! own sake: when this provider points at a sandbox or a remote machine, all five tools move
//! with it and none of them needs editing. That is the one thing that makes "swapping a
//! provider changes the product" true rather than a slogan.

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

/// This machine's own disk.
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
        // By here we know there is no NUL byte; the rest can still be broken UTF-8, and
        // replacement characters are then correct: the file *is* text, with a few odd bytes.
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
