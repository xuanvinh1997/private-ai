//! Language Server Protocol bridge: the code questions syntax alone cannot answer.
//! `pai-index` owns tree-sitter lookups; this crate owns the four operations that need a
//! compiler, and registers no tool at all when no server is found on `PATH`.

pub mod client;
pub mod config;
pub mod error;
pub mod launch;
pub mod plugin;
pub mod pool;
pub mod proto;
pub mod seam;
pub mod tool;
pub mod uri;

pub use client::Client;
pub use config::{LanguageConfig, Limits, defaults, language_id};
pub use error::LspError;
pub use launch::{Channel, ChildLaunch, Launch, locate};
pub use plugin::LspPlugin;
pub use pool::{Entry, StdioServers};
pub use seam::{Answer, Hit, LanguageServers, Lsp, Note, Operation, Query};
pub use tool::{LspArgs, LspTool};
pub use uri::{UriError, from_uri, to_uri};
