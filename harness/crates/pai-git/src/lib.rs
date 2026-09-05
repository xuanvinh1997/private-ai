//! Reading a git repository: status, diff, log, show, blame.
//!
//! # Why a child process and not `gix`
//!
//! Every command here shells out to the `git` binary through `tokio::process::Command`,
//! never through a shell. The repository already made this call once — see
//! `pai-project/src/clone.rs`, which spawns `git clone` for the same reason — and being
//! consistent with it is worth more than the alternative's purity:
//!
//! * `gix` is a large dependency to compile and to keep current, and its porcelain is
//!   deliberately incomplete. `blame` and rename-detecting `diff` in particular are areas
//!   where it does not yet match what `git` prints, and a model comparing our `blame`
//!   against the one the user sees in their terminal must not find two different answers.
//! * The user's own configuration — `diff.algorithm`, `mailmap`, `core.excludesFile`,
//!   worktrees, submodules, LFS smudge filters — is honoured for free. Reimplementing that
//!   surface is how a "pure Rust" answer quietly becomes a wrong answer.
//! * The cost is a runtime dependency on a binary that may be missing. That is handled, not
//!   ignored: a missing `git` becomes one clear Vietnamese sentence naming PATH, and the
//!   tests skip themselves rather than fail red on a machine without it.
//!
//! The trade the other way is real and worth naming: we pay a process spawn per call, we
//! parse text meant for humans, and we inherit whatever the user's `git` version prints.
//! [`repo`] pins that down as far as it can — `LC_ALL=C` for stable English, `--no-pager`,
//! `core.quotepath=false`, and machine-oriented output formats everywhere one exists.
//!
//! # Why read-only, and only read-only
//!
//! There is no `git.commit`, `git.push`, `git.checkout`, `git.reset` or anything else that
//! writes. This is a boundary, not an omission:
//!
//! * Git history is the user's undo stack. A tool that rewrites it can destroy work no
//!   other tool in this application can destroy, and it would do so under a `ToolMeta` the
//!   model never sees and cannot be argued out of.
//! * The application already has `bash`. A write to git is therefore not *impossible*, it
//!   is merely *visible*: it goes through the shell tool's approval, where the user reads
//!   the exact command before it runs. Adding `git.commit` would move that same action to a
//!   path with a friendlier name and a thinner prompt, which is a downgrade dressed as a
//!   feature.
//! * Because everything here is read-only, all five tools declare
//!   [`pai_tools::ToolMeta::read_only`] and stay available to a read-only agent. One
//!   mutating tool in the set would be a reason to distrust the whole plugin.
//!
//! Adding a writing tool later is a separate discussion with a separate design, not a
//! patch to this crate.

pub mod error;
pub mod plugin;
pub mod render;
pub mod repo;
pub mod tools;

pub use error::GitError;
pub use plugin::GitPlugin;
pub use repo::{GitOutput, Repo};
