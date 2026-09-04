//! Document library seam: the types the layers above see, and their contract.
//! The default implementation is in-process Rust; expensive work and optional extractor
//! fallbacks remain asynchronous so callers never block a Tauri runtime thread.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_core::ServiceKey;
use serde::{Deserialize, Serialize};

use crate::error::RagError;
use crate::format::Format;
use crate::search::MatchedBy;

/// How many files one scan will ingest; kept here because the UI names the number too.
pub const MAX_FILES: usize = 5_000;

/// A document as the layers above see it. Maps one-to-one onto `DocumentView` in `app/`.
#[derive(Clone, Debug)]
pub struct Document {
    pub id: String,
    /// The real file in the project folder; this is what the user can open in a file browser.
    pub path: PathBuf,
    /// Where the file came from. Equal to `path` for files already inside the project folder.
    pub origin: String,
    pub title: String,
    pub format: Format,
    pub bytes: u64,
    pub chunks: u32,
    pub embedded: bool,
    pub added_at: i64,
    /// `None` plus `embedded == false` means queued, not broken.
    pub error: Option<String>,
    /// Page count, when the format has such a notion.
    pub pages: u32,
    /// Which pages needed OCR; the UI says "12/40 pages via OCR", which explains a slow ingest.
    pub ocr_pages: Vec<u32>,
}

/// A scan in flight, enough for the UI to say "scanning 12/240 files".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scanning {
    pub done: u32,
    pub total: u32,
}

/// Library health. Maps one-to-one onto `LibraryStats` in `app/`.
#[derive(Clone, Debug)]
pub struct Stats {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
    pub embedder: Option<String>,
    pub semantic_ready: bool,
    /// Explanation shown when `semantic_ready` is false; the only place the user learns why results are keyword-only.
    pub reason: Option<String>,
    /// The user's document folder; the UI must be able to show it when no files turn up.
    pub root: PathBuf,
    pub files_seen: u32,
    pub files_skipped: u32,
    /// Files that were tried and could not be read.
    pub unreadable: u32,
    pub excluded: u32,
    pub scanned_at: Option<i64>,
    pub scanning: Option<Scanning>,
}

/// A matching chunk. Maps one-to-one onto `DocumentHit` in `app/`.
#[derive(Clone, Debug)]
pub struct Hit {
    pub document_id: String,
    pub title: String,
    pub path: PathBuf,
    pub ordinal: u32,
    pub heading: Option<String>,
    pub text: String,
    pub score: f32,
    pub matched_by: MatchedBy,
    /// Page holding this chunk, `0` when the format has no pages; it goes into the citation.
    pub page: u32,
}

/// Stage of a file during ingest. Maps onto `IngestProgress.stage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStage {
    Reading,
    /// Page-level optical character recognition for scanned PDFs and images.
    Ocr,
    Stored,
    Failed,
    /// Skipped for a reason: too large, or past the file cap. Distinct from `Failed` - the file is fine, the library refused it.
    Skipped,
    Removed,
    /// Catch-up embedding pass at the end of a run; kept out of `Failed` so the UI does not count it as a broken *file*.
    Embedding,
    /// The user stopped the batch. No further work is scheduled and completed files remain available.
    Cancelled,
    /// The whole batch completed normally. Always the last event of a successful stream.
    Finished,
}

impl IngestStage {
    pub fn as_str(self) -> &'static str {
        match self {
            IngestStage::Reading => "reading",
            IngestStage::Ocr => "ocr",
            IngestStage::Stored => "stored",
            IngestStage::Failed => "failed",
            IngestStage::Skipped => "skipped",
            IngestStage::Removed => "removed",
            IngestStage::Embedding => "embedding",
            IngestStage::Cancelled => "cancelled",
            IngestStage::Finished => "finished",
        }
    }
}

/// One progress tick. Maps onto `IngestProgress` in `app/`.
#[derive(Clone, Debug)]
pub struct IngestEvent {
    pub path: String,
    pub stage: IngestStage,
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    pub error: Option<String>,
    /// The document just finished, so the UI can append a row without refetching the whole list.
    pub document: Option<Document>,
}

/// What the tools and the layers above see; `sync`, `ingest` and `remove` are UI commands rather than model tools, but they stay on the seam so there is only one path to the service.
#[async_trait]
pub trait DocLibrary: Send + Sync + 'static {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError>;
    async fn documents(&self) -> Result<Vec<Document>, RagError>;
    /// Read a document straight through, chunk by chunk.
    async fn chunks(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Hit>, RagError>;
    async fn stats(&self) -> Result<Stats, RagError>;
    /// Catch up with the project folder. The main entry point for a document project.
    fn sync(&self) -> BoxStream<'_, IngestEvent>;
    /// Ingest a specific list of files; the stream borrows `&self` so it cannot outlive the library.
    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent>;
    /// Forget every fingerprint and read the whole folder again.
    fn reprocess(&self) -> BoxStream<'_, IngestEvent>;
    /// Stop the active ingest pass, if any. Work already committed stays in the library.
    fn cancel_ingest(&self) -> bool;
    /// Drop a document from the library. Does *not* delete the file on disk.
    async fn remove(&self, id: &str) -> Result<(), RagError>;
}

/// Document library seam.
pub enum Docs {}
impl ServiceKey for Docs {
    type Api = dyn DocLibrary;
    const NAME: &'static str = "rag.docs";
}
