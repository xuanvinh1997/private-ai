//! Three modes and one definition of where writes are allowed: [`writable_roots`], asked by
//! SBPL, Landlock and `pai-fs` alike, since two answers that drift apart are a hole.
//! Paths are canonicalised first, or `/tmp` opens a directory the kernel never matches.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The confinement mode; wire names are fixed so config and the session journal agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// No writes at all, workspace included; only the mandatory holes remain (`/dev/null`).
    ReadOnly,
    /// Writes allowed in the workspace and the temp directory, refused everywhere else.
    WorkspaceWrite,
    /// No confinement at all: the absence of a sandbox, not a configuration of one.
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

    /// Does this mode ask for a boundary at all; `danger-full-access` skips every backend.
    pub fn confining(self) -> bool {
        !matches!(self, Mode::DangerFullAccess)
    }
}

/// The policy for one run; `workspace_root` is the session's directory, never the command's `cwd`.
#[derive(Debug, Clone)]
pub struct Policy {
    pub mode: Mode,
    pub workspace_root: PathBuf,
    /// Cut the process off from the network; off by default, and TCP-only on Linux.
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

    /// The same policy with the network cut off; a builder, so asking for it is explicit.
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

/// Writable roots, canonicalised and deduplicated; unresolvable paths are dropped, and an empty list means the question does not apply.
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

/// Drop duplicate and nested roots, so a profile never lists the same area twice.
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

/// Does this path sit inside a writable root; for the in-process guardrail and tests only.
pub fn is_writable(policy: &Policy, path: &Path) -> bool {
    if policy.mode == Mode::DangerFullAccess {
        return true;
    }
    writable_roots(policy)
        .iter()
        .any(|root| path.starts_with(root))
}
