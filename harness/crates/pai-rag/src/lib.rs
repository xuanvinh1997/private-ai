//! Document library: ingest many file types, then ask questions over them.
//! The implementation lives in `services/rag/` (Python, MCP over stdio) because format
//! readers, OCR and cross-encoder reranking are far ahead there; this crate is the client.

pub mod client;
pub mod error;
pub mod format;
pub mod library;
pub mod plugin;
pub mod search;
pub mod sidecar;
pub mod tools;

pub use client::RagClient;
pub use error::RagError;
pub use format::Format;
pub use library::{
    DocLibrary, Docs, Document, Hit, IngestEvent, IngestStage, MAX_FILES, Scanning, Stats,
};
pub use plugin::RagPlugin;
pub use search::MatchedBy;
pub use sidecar::{Sidecar, SidecarConfig};
