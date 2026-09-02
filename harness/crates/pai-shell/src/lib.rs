//! Command execution: one seam, four tools, one guard.
//!
//! Two things worth remembering:
//!
//! **What we run is a process tree.** `sh -c "npm test"` spawns `npm`, which spawns
//! `node`. Killing the shell leaves both of those holding ports and file locks. So every
//! command runs in its own process group and every signal goes to the whole group. See
//! [`provider`].
//!
//! **There is no blocklist.** Filtering dangerous commands by string matching always leaks,
//! and what it produces is not safety but the feeling of safety — the thing that gets
//! people to click "allow" without reading. The real defences are approval (here) and
//! confining the process (`pai-sandbox`).

pub mod jobs;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use jobs::{Job, JobState, Jobs};
pub use plugin::ShellPlugin;
pub use provider::{Execution, LocalShell, Request, Shell, ShellError, ShellExecutor};
