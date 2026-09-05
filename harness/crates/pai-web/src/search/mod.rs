//! Web search, behind a seam.
//!
//! The provider is a trait for the same reason `pai-rag` keeps `Arc<dyn DocLibrary>`: today it is
//! Brave, tomorrow it is whatever the user has a key for, and the tool above must not care. The
//! seam pays for itself twice, because it is also where the tests plug a fake in and so never need
//! a network or a key to prove the tool renders results correctly.

pub mod brave;

use async_trait::async_trait;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

pub use brave::Brave;

/// One result. Deliberately the smallest useful shape: a model that wants the page body has
/// `web.fetch` for that, and pulling full pages into a result list would blow the budget.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    /// The provider's own excerpt, already flattened out of HTML.
    pub snippet: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Told in full: a silent empty result would look like "the web has nothing on this", which is
    /// a very different answer from "this machine has no key".
    #[error(
        "chưa có khoá API cho nhà cung cấp tìm kiếm `{provider}`. \
         Đặt biến môi trường `{env}` rồi mở lại ứng dụng."
    )]
    MissingKey {
        provider: &'static str,
        env: &'static str,
    },
    #[error("nhà cung cấp tìm kiếm `{provider}` trả về HTTP {status}")]
    Status {
        provider: &'static str,
        status: u16,
    },
    #[error("lỗi mạng khi gọi nhà cung cấp tìm kiếm: {0}")]
    Transport(String),
    #[error("không đọc được kết quả từ nhà cung cấp tìm kiếm: {0}")]
    Malformed(String),
    #[error("đã huỷ trước khi có kết quả")]
    Cancelled,
}

/// Somewhere to send a query.
#[async_trait]
pub trait SearchProvider: Send + Sync + 'static {
    /// Shown to the user in errors, so it should be the name they would recognise.
    fn name(&self) -> &'static str;

    /// `limit` is a request, not a contract: a provider may return fewer, and callers must cope.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<SearchHit>, SearchError>;
}
