//! Command execution: one seam, four tools, one guard. What runs is a process tree, so every
//! command gets its own process group and signals go to the whole group. There is no blocklist:
//! string matching only feels safe; the real defences are approval and `pai-sandbox`.

pub mod jobs;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use jobs::{Job, JobState, Jobs};
pub use plugin::ShellPlugin;
pub use provider::{Execution, LocalShell, Request, Shell, ShellError, ShellExecutor};
