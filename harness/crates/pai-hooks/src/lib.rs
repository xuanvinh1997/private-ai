//! Hooks: operator policy, run around every tool call.
//!
//! A hook is an external command. The harness hands it a description of the call on stdin
//! as JSON and reads the decision from stdout, also JSON. Policy can therefore be written
//! in anything — a one-line `jq`, a Python script, a company binary — without anyone
//! rebuilding the application.
//!
//! Three decisions worth writing down:
//!
//! **Hooks run *outside* the sandbox.** They do not go through the `Shell` seam; they are
//! spawned directly. A hook is the operator's policy, not the model's work, and letting
//! the agent's sandbox decide whether policy gets to run inverts the relationship. The
//! price is that a hook runs with the user's full privileges — but it was already a
//! command the user wrote into their own config file.
//!
//! **A broken hook allows; a hook that says no denies.** A hook with a syntax error, a
//! timeout, or a missing file is a **failure of the policy**, not evidence that the call
//! is dangerous; blocking everything because one script broke turns a typo into a frozen
//! application. Conversely, a hook that runs and says `deny` is respected absolutely.
//!
//! This is a deliberate **difference** from approval: `Approver` is fail-closed because it
//! speaks for a person sitting there, and hooks are fail-open because they speak for a
//! config file. Different roles, so different defaults.
//!
//! **Hooks cannot rewrite arguments.** They return `allow` or `deny` with a reason, and
//! nothing else. Allowing rewrites sounds convenient, but it produces a call that neither
//! the model nor the user ever saw — and the transcript would then lie about what actually
//! ran.

pub mod plugin;
pub mod runner;

pub use plugin::{HookConfig, HooksPlugin};
pub use runner::{HookDecision, HookInput};
