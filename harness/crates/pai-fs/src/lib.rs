//! The filesystem: one seam, one policy, five tools. Canonicalise first, then check.
//! Policy is middleware, not tool code, so it also covers tools written later.
//! Tools go through [`provider::Fs`], so repointing the provider moves them all.

pub mod observed;
pub mod path;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use observed::{ReadBeforeEdit, ReadLedger};
pub use path::{FileRoots, PathError, looks_binary};
pub use plugin::FsPlugin;
pub use provider::{Fs, FsError, FsProvider, LocalFs};
