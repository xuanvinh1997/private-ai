//! Projects: a directory, and everything attached to it.
//! Switching projects tears down and remounts the project-tier plugin branch; there is no
//! separate reconfigure path. A project's identity is its canonical path, not its name.
mod clone;
mod store;

pub use clone::{CloneError, CloneEvent, CloneRequest, clone};
pub use store::{Project, ProjectError, ProjectKind, ProjectStore, SqliteProjectStore};
