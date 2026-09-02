//! Projects: a directory, and everything attached to it.
//!
//! Before this file, the application had exactly **one** working directory, fixed at
//! startup from an environment variable. Seven plugins captured that value at construction
//! time, and there was no way to change it. A coding agent like that is usable for one repo
//! per launch.
//!
//! Projects are the answer, and the interesting part is not in this crate but in **how
//! switching projects is implemented**: the project-tier branch of the plugin tree is torn
//! down and mounted again with the new path. There is no parallel "reconfigure everything"
//! path — needing to write one would mean the plugin architecture was wrong from the start.
//! See `Harness::open_project` in `pai-app`.
//!
//! **A project's identity is its canonical path**, not its name. Two ways into the same
//! directory — through a symlink, through `..` — have to be one project, or the user ends
//! up with two rows in the list pointing at the same place, each remembering half the
//! history.
mod clone;
mod store;

pub use clone::{CloneError, CloneEvent, CloneRequest, clone};
pub use store::{Project, ProjectError, ProjectKind, ProjectStore, SqliteProjectStore};
