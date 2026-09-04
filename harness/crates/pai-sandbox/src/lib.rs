//! Process confinement: one seam, three modes, one implementation per operating system.
//! It governs file writes, and the network only when asked; reads are never restricted.
//! [`Enforcement`] reports the truth about this machine, it never promises a boundary.

pub mod plugin;
pub mod policy;
pub mod seam;

#[cfg(target_os = "macos")]
pub mod seatbelt;

#[cfg(target_os = "linux")]
pub mod landlock;

pub mod unconfined;

pub use plugin::SandboxPlugin;
pub use policy::{Mode, Policy, writable_roots};
pub use seam::{Enforcement, Sandbox, SandboxError, SandboxProvider};
pub use unconfined::Unconfined;
