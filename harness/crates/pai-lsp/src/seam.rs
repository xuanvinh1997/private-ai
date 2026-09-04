//! This crate's seam, and the vocabulary that crosses it.
//! One seam: can anything answer semantic questions about code? Coordinates here are
//! 1-based by character, and the conversion to LSP's 0-based/UTF-16 stays in the provider.

use std::path::PathBuf;

use async_trait::async_trait;
use pai_core::ServiceKey;

use crate::error::LspError;

/// Four operations, all of them things tree-sitter cannot do; keeping the boundary with `pai-index` means the model is never left choosing between two tools for one question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Definition,
    References,
    Hover,
    Diagnostics,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Definition => "definition",
            Operation::References => "references",
            Operation::Hover => "hover",
            Operation::Diagnostics => "diagnostics",
        }
    }

    /// `diagnostics` is about a whole file, the other three about a cursor; distinguished here so argument checking need not repeat the list.
    pub fn needs_position(self) -> bool {
        !matches!(self, Operation::Diagnostics)
    }
}

/// One question: which operation, in which file, at which cursor (1-based).
#[derive(Clone, Debug)]
pub struct Query {
    pub op: Operation,
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
}

/// A place in the code, in the coordinates humans and `read` share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Relative to the working directory when inside it, absolute when not.
    pub path: String,
    pub line: u32,
    pub column: u32,
    /// The line of code there, trimmed; empty when the file is outside the working directory, since `pai-fs` boundaries have no exception for this crate.
    pub text: String,
    /// Can `read` reach it? Stated so the model does not go read a file it will be refused.
    pub reachable: bool,
}

/// One compiler diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub line: u32,
    pub column: u32,
    pub severity: &'static str,
    pub source: Option<String>,
    pub message: String,
}

/// The answer, plus one thing the server says about itself: `busy` means still indexing, and it rides along with every answer shape, because a partial reference list must not read as complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Locations {
        hits: Vec<Hit>,
        truncated: bool,
        busy: bool,
    },
    Hover {
        text: String,
        busy: bool,
    },
    Diagnostics {
        notes: Vec<Note>,
        busy: bool,
    },
}

#[async_trait]
pub trait LanguageServers: Send + Sync + 'static {
    /// Which languages actually have a server on this machine; an empty list means the plugin registered no tool.
    fn languages(&self) -> Vec<String>;

    async fn ask(&self, query: &Query) -> Result<Answer, LspError>;
}

/// No provider means no `lsp` tool, and everything else still works.
pub enum Lsp {}
impl ServiceKey for Lsp {
    type Api = dyn LanguageServers;
    const NAME: &'static str = "lsp";
}
