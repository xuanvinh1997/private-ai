//! Persistent terminal: one seam, six tools, sessions that outlive the turn that opened them.
//! A real PTY, not a pipe, so tools behave the way they do for a human; sessions are owner-scoped,
//! die with the plugin, and buffer only the newest lines while reporting how many were dropped.

pub mod buffer;
pub mod plugin;
pub mod provider;
pub mod seam;
pub mod session;
pub mod tools;

pub use buffer::{Page, Ring};
pub use plugin::{TerminalPlugin, register_tools};
pub use provider::{LocalTerminals, SHELL_BACKEND};
pub use seam::{
    DEFAULT_COLS, DEFAULT_MAX_LINES, DEFAULT_ROWS, OpenRequest, Owner, Sent, SessionInfo, Signal,
    Stop, TerminalError, TerminalHost, Terminals, Wait,
};
pub use tools::{
    TerminalClose, TerminalList, TerminalOpen, TerminalRead, TerminalSend, TerminalSignal,
};
