//! Process confinement: one seam, three modes, one implementation per operating system.
//!
//! The four statements below are the entire content of this crate, and all four are
//! negative.
//!
//! **The sandbox governs file effects, and the network only when asked.** No mode blocks the
//! network *by default*, and that default stands: a blanket deny breaks `cargo` and `npm`
//! badly enough that an always-on version would make the agent useless. What changed is that
//! the choice now exists — [`Policy::deny_network`] cuts a process off, and reading a repo
//! someone sent you is a different job from building your own.
//!
//! What it covers is **not the same on both platforms**, and rounding that off would be the
//! same lie this crate refuses everywhere else:
//!
//! | | Covers | How |
//! |---|---|---|
//! | macOS | everything, TCP and UDP | `(deny network*)` in the SBPL profile |
//! | Linux | **TCP only**, from Landlock ABI 4 | handle the net access, grant no port |
//!
//! Landlock has no UDP verb, so on Linux a confined process can still resolve names and
//! still send over UDP. A socket already connected when the ruleset is applied also stays
//! usable. Ask [`seam::SandboxProvider::network_confinable`] before relying on any of it:
//! below ABI 4, and on Windows, it answers false rather than accepting the flag and building
//! nothing — a caller told "no" is better off than one silently given an empty boundary.
//!
//! **The two platforms do not confine identically.** macOS opens exactly one hole for
//! `/dev/null`; Linux opens the whole `/dev` directory at the file-permission level, because
//! Landlock does not govern device nodes per file. Neither can create or delete entries in
//! `/dev`. A table saying "both are `Full`" without saying where they differ lies by
//! omission.
//!
//! **The sandbox does not govern *reads*.** All three modes allow reading the whole machine.
//! A coding agent has to read the repo, the toolchain, the dependency cache and the git
//! config; punching enough holes to make that work leaves the read boundary meaningless.
//! Secrets in `~/.ssh` are still readable — what blocks those is `pai-fs`'s protected-path
//! list, not this crate.
//!
//! **The sandbox does not claim to be confining.** [`Enforcement`] is *reported truth*, not
//! a promise. A lying sandbox is more dangerous than none: the user clicks "allow" because
//! they believe there is a boundary. Which is why every provider here returns `None` with a
//! reason rather than returning `Full` to be safe.
//!
//! # Operating-system map
//!
//! | | Blocks writes outside the workspace | How |
//! |---|---|---|
//! | macOS | yes, `Full` | `sandbox-exec` with a generated SBPL profile ([`seatbelt`]) |
//! | Linux | yes, `Full`/`Partial` by kernel ABI | Landlock via a helper binary ([`landlock`]) |
//! | Windows | not yet | [`Enforcement::None`] with a reason ([`unconfined`]) |
//!
//! Elsewhere the provider is also a `None` with a reason — rather than no provider at all,
//! because "nobody answered" and "confinement is unavailable" are two different sentences as
//! far as the approval dialog is concerned.

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
