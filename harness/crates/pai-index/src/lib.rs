//! Code index: tree-sitter for symbols, SQLite + FTS5 for lookup.
//! Syntax and symbols only, no embeddings and no model, so it never goes silent when the
//! model is down and stays cheap enough to refresh incrementally before every question.

pub mod complete;
pub mod error;
pub mod extract;
pub mod graph;
pub mod index;
pub mod lang;
pub mod plugin;
pub mod store;
pub mod symbol;
pub mod tools;

pub use error::IndexError;
pub use extract::{Extraction, Extractor};
pub use graph::{
    CentralSymbol, DirectorySummary, EdgeKind, GraphEdge, GraphNode, NAME_BASED_NOTICE,
    Neighborhood, Overview, Reference, Stats,
};
pub use index::{CodeIndex, Index, MAX_DEPTH, MAX_NODES, MAX_PATHS, SymbolIndex, SyncReport};
pub use lang::{LANGUAGES, Lang};
pub use plugin::IndexPlugin;
pub use store::Store;
pub use symbol::{Symbol, SymbolKind};
