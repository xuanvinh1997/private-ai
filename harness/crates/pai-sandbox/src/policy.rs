//! Three modes, and **one** definition of "where writes are allowed".
//!
//! [`writable_roots`] is the only function that answers that question. macOS's SBPL
//! profile, Linux's Landlock ruleset and `pai-fs`'s in-process guardrail all have to ask
//! it, because two places answering separately and then drifting apart is the exact shape
//! of a hole: the user sees the `write` tool refuse a path, concludes the boundary is real,
//! and then a `bash` command writes to that same path.
//!
//! Paths are **canonicalised** before entering the list. This is not tidiness: Seatbelt
//! matches on resolved paths, so `/tmp` — which is really `/private/tmp` on macOS — without
//! canonicalisation allows a directory nobody touches, while the boundary still looks like
//! it opened the right place.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The confinement mode. Wire names are kept from dsh so config and the session journal
/// stay interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// No writes at all, workspace included. Only the mandatory holes remain
    /// (`/dev/null`).
    ReadOnly,
    /// Writes allowed in the workspace and in the temp directory. Everywhere else is
    /// refused.
    WorkspaceWrite,
    /// No confinement at all. This is the *absence* of a sandbox, not a configuration of
    /// one.
    DangerFullAccess,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::ReadOnly => "read-only",
            Mode::WorkspaceWrite => "workspace-write",
            Mode::DangerFullAccess => "danger-full-access",
        }
    }

    /// Does this mode ask for a boundary at all.
    ///
    /// `danger-full-access` returns `false`, which is why it never goes through any
    /// backend: wrapping argv in a runner and then granting it everything only adds a
    /// process to the tree and another error dialect to guess at, in exchange for nothing.
    pub fn confining(self) -> bool {
        !matches!(self, Mode::DangerFullAccess)
    }
}

/// The policy for **one** run.
///
/// `workspace_root` is the session's fixed working directory, not the command's `cwd`.
/// Taking `cwd` as the root would confine a command running in a subdirectory more tightly
/// than the policy the user approved, and one running in a parent directory more loosely.
#[derive(Debug, Clone)]
pub struct Policy {
    pub mode: Mode,
    pub workspace_root: PathBuf,
    /// Cut the process off from the network. **Off by default, and that stays true.**
    ///
    /// The default is off for a reason that has not changed: on macOS a blanket network
    /// deny breaks `cargo` and `npm`, so a coding agent with it always on is a coding agent
    /// that cannot fetch a dependency. What was missing was not the default — it was the
    /// *choice*. Reading a repo someone sent you is a different job from building your own,
    /// and only one of them needs to reach the internet.
    ///
    /// What it covers differs by platform, and the difference is not a detail:
    ///
    /// - **macOS** — `(deny network*)` covers everything, TCP and UDP alike.
    /// - **Linux** — Landlock from ABI 4, and **TCP only**. There is no UDP verb, so DNS and
    ///   any UDP transport still leave the box. A socket already connected when the ruleset
    ///   is applied also stays usable.
    ///
    /// Ask [`crate::seam::SandboxProvider::network_confinable`] before relying on it: a
    /// provider that cannot honour the flag says so instead of accepting it silently.
    pub deny_network: bool,
}

impl Policy {
    pub fn new(mode: Mode, workspace_root: impl Into<PathBuf>) -> Policy {
        Policy {
            mode,
            workspace_root: workspace_root.into(),
            deny_network: false,
        }
    }

    /// The same policy, with the network cut off.
    ///
    /// Deliberately a builder rather than an argument to [`Policy::new`]: every existing
    /// caller keeps the behaviour it already had, and asking for confinement is something a
    /// caller has to *write down*.
    pub fn deny_network(mut self) -> Policy {
        self.deny_network = true;
        self
    }

    pub fn read_only(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::ReadOnly, workspace_root)
    }

    pub fn workspace_write(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::WorkspaceWrite, workspace_root)
    }

    pub fn danger_full_access(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::DangerFullAccess, workspace_root)
    }
}

/// The roots this mode allows writing to, canonicalised and deduplicated.
///
/// A path that does not exist is **dropped** rather than kept verbatim: failing to
/// canonicalise means not knowing where it really points, and granting write access to an
/// unresolved string grants it to whatever later takes that place.
///
/// `danger-full-access` returns an empty list, and that does not mean "writes are allowed
/// nowhere" — it means "this question does not apply to that mode". Callers have to check
/// [`Mode::confining`] first.
pub fn writable_roots(policy: &Policy) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if policy.mode == Mode::WorkspaceWrite {
        let temp = std::env::temp_dir();
        for candidate in [
            policy.workspace_root.clone(),
            temp,
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
        ] {
            if let Ok(resolved) = candidate.canonicalize() {
                roots.push(resolved);
            }
        }
    }
    dedup_nested(roots)
}

/// Drop duplicates and roots already contained in another root.
///
/// Not for brevity: `/tmp` and `/private/tmp` canonicalise to the same place on macOS, and
/// an SBPL profile listing the same `subpath` twice makes whoever reads it believe there
/// are two distinct areas.
fn dedup_nested(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort();
    roots.dedup();
    let picked: Vec<PathBuf> = roots
        .iter()
        .filter(|root| {
            !roots
                .iter()
                .any(|other| other.as_path() != root.as_path() && root.starts_with(other))
        })
        .cloned()
        .collect();
    picked
}

/// Does this path sit inside a writable root.
///
/// For the in-process guardrail and for tests; the out-of-process backends do not use it,
/// because they hand the matching to the kernel.
pub fn is_writable(policy: &Policy, path: &Path) -> bool {
    if policy.mode == Mode::DangerFullAccess {
        return true;
    }
    writable_roots(policy)
        .iter()
        .any(|root| path.starts_with(root))
}
