//! Document library: ingest many file types, then ask questions over them.
//! The implementation is native Rust and runs in-process.

pub mod error;
pub mod format;
pub mod library;
pub mod native;
pub mod plugin;
pub mod search;
pub mod tools;

pub use error::RagError;
pub use format::Format;
pub use library::{
    DocLibrary, Docs, Document, Hit, IngestEvent, IngestStage, MAX_FILES, Scanning, Stats,
};
pub use native::{NativeLibrary, purge_library};
pub use plugin::RagPlugin;
pub use search::MatchedBy;
