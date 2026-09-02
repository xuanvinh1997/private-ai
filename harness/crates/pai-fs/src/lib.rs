//! The filesystem: one seam, one policy, five tools.
//!
//! Three things worth remembering:
//!
//! **Canonicalise first, check second.** Checking before canonicalising lets
//! `root/../../etc/passwd` through, because at comparison time it still starts with
//! `root/`. See [`path`].
//!
//! **Policy does not live inside the tools.** The read-before-edit rule is a middleware on
//! the `pai-tools` pipeline, so `edit` does not know the rule exists, disabling it is
//! removing a plugin, and any file-writing tool written later is covered automatically.
//! See [`observed`].
//!
//! **Tools do not call `std::fs`.** They go through [`provider::Fs`], so pointing the
//! provider at a sandbox moves all five tools with it and none of them needs editing.

pub mod observed;
pub mod path;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use observed::{ReadBeforeEdit, ReadLedger};
pub use path::{FileRoots, PathError, looks_binary};
pub use plugin::FsPlugin;
pub use provider::{Fs, FsError, FsProvider, LocalFs};
