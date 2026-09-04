//! Hooks: operator policy as an external command, JSON in on stdin, a decision out on stdout.
//! They run outside the sandbox, they are fail-open (a broken hook allows, unlike `Approver`),
//! and they cannot rewrite arguments, so the transcript matches what actually ran.

pub mod plugin;
pub mod runner;

pub use plugin::{HookConfig, HooksPlugin};
pub use runner::{HookDecision, HookInput};
