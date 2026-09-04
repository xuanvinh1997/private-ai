//! Native core for document RAG.
//!
//! This crate intentionally has no MCP, model, or extraction dependencies. It owns the
//! deterministic parts of the pipeline, including the SQLite schema and ranking rules.

pub mod chunking;
pub mod fusion;
pub mod store;

pub use chunking::{Chunk, DEFAULT_SECTION, SectionAwareSplitter, embedding_text_for};
pub use fusion::{MatchedBy, RRF_K, Ranked, fuse};
pub use store::{
    ChunkRow, DocumentInput, DocumentRow, Identity, Stats, Store, StoreError, StoreResult,
};
