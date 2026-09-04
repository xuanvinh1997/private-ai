//! Terminal seam: the vocabulary plus the trait an implementation must provide.
//! [`Owner`] is the first parameter everywhere on purpose: ownership is part of the question,
//! not an external filter a second implementation could forget to apply.

use std::path::PathBuf;

use async_trait::async_trait;
use pai_core::{ScopeKey, ServiceKey};

/// Default size of a new session: 80x24, the default every CLI tool is tested against, so wrapping surprises least.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// Buffer cap per session in lines: enough for a whole failed build, not enough for an overnight dev server to eat memory.
pub const DEFAULT_MAX_LINES: usize = 5_000;

/// Session owner: `pai-core`'s scope, `None` being the host. Reusing the core type avoids two identity systems drifting apart.
pub type Owner = Option<ScopeKey>;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    /// Same message for "missing" and "someone else's", deliberately -- see [`TerminalHost`].
    #[error("không có phiên terminal `{0}`")]
    NoSession(String),
    #[error("không có backend terminal `{0}`; đang có: {1}")]
    NoBackend(String, String),
    #[error("không mở được phiên: {0}")]
    Spawn(String),
    #[error("phiên `{0}` đã kết thúc")]
    Ended(String),
    #[error("không ghi được vào phiên: {0}")]
    Write(String),
    /// Some signals only make sense aimed at a process other than the session shell.
    #[error("{0}")]
    Refused(String),
}

/// Signals an agent may send; a closed enum so killing the session shell has to go through [`TerminalHost::close`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Int,
    Term,
    Kill,
    Tstp,
    Hup,
}

impl Signal {
    pub fn parse(name: &str) -> Option<Signal> {
        match name {
            "SIGINT" => Some(Signal::Int),
            "SIGTERM" => Some(Signal::Term),
            "SIGKILL" => Some(Signal::Kill),
            "SIGTSTP" => Some(Signal::Tstp),
            "SIGHUP" => Some(Signal::Hup),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Int => "SIGINT",
            Signal::Term => "SIGTERM",
            Signal::Kill => "SIGKILL",
            Signal::Tstp => "SIGTSTP",
            Signal::Hup => "SIGHUP",
        }
    }
}

/// Request to open a session.
pub struct OpenRequest {
    /// Registered backend. Currently only `shell`.
    pub backend: String,
    /// Display name, local to the owner. `None` uses the shortened id.
    pub name: Option<String>,
    /// `None` uses the implementation's workspace root.
    pub cwd: Option<PathBuf>,
}

/// Snapshot of a session, enough for the UI and the model to talk about it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub cwd: String,
    pub rows: u16,
    pub cols: u16,
    /// Whether the session shell is alive; a dead session stays readable until closed, its last output matters most.
    pub alive: bool,
}

/// How to wait after a send. Quiet time, not prompt detection: `PS1` is arbitrary and a REPL has its own prompt.
#[derive(Clone, Copy, Debug)]
pub struct Wait {
    /// Nothing new for this long counts as settled.
    pub quiet: std::time::Duration,
    /// Absolute cap, so a never-quiet command cannot hold a turn forever.
    pub timeout: std::time::Duration,
}

/// Why the wait stopped; surfaced to the model, since "not done" and "done" are different conclusions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// No wait: send and return immediately.
    Background,
    Quiet,
    Timeout,
    Ended,
}

/// Result of one send.
pub struct Sent {
    pub lines: Vec<String>,
    /// Lines dropped from the buffer since the session opened.
    pub dropped: usize,
    pub stopped: Stop,
}

/// Seam implementation. An unknown id and another owner's id return the same error, so lookup cannot be used as a probe.
#[async_trait]
pub trait TerminalHost: Send + Sync + 'static {
    async fn open(&self, owner: Owner, req: OpenRequest) -> Result<SessionInfo, TerminalError>;

    fn list(&self, owner: Owner) -> Vec<SessionInfo>;

    fn info(&self, owner: Owner, id: &str) -> Result<SessionInfo, TerminalError>;

    /// Write bytes to the PTY, then optionally wait. The caller owns the trailing `\n`, and waiting lives here
    /// rather than in the tool because only the implementation sees the byte stream.
    async fn send(
        &self,
        owner: Owner,
        id: &str,
        bytes: &[u8],
        wait: Option<Wait>,
    ) -> Result<Sent, TerminalError>;

    /// Read a page from the buffer; `offset` counts back from the newest line.
    fn read(
        &self,
        owner: Owner,
        id: &str,
        offset: usize,
        count: usize,
    ) -> Result<crate::buffer::Page, TerminalError>;

    /// Resize the window. No tool calls it, but a UI embedding a session in a resizable pane needs a way to send `SIGWINCH`.
    fn resize(&self, owner: Owner, id: &str, rows: u16, cols: u16) -> Result<(), TerminalError>;

    /// Signal the session's foreground process group.
    fn signal(&self, owner: Owner, id: &str, signal: Signal) -> Result<(), TerminalError>;

    /// Close a session and wait for its process tree to disappear.
    async fn close(&self, owner: Owner, id: &str) -> Result<(), TerminalError>;

    /// Close everything, ignoring ownership. Plugin teardown only.
    async fn close_all(&self);
}

/// Seam.
pub enum Terminals {}

impl ServiceKey for Terminals {
    type Api = dyn TerminalHost;
    const NAME: &'static str = "terminals";
}
