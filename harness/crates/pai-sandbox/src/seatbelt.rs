//! macOS: `sandbox-exec` with a generated SBPL profile.
//!
//! **Built on something Apple marked deprecated.** `sandbox-exec` has carried a deprecation
//! warning since macOS 10.7, and so has `sandbox.h`; as of 2026 it still works, and App
//! Sandbox itself still rests on the same machinery. No public API replaces it for
//! confining an arbitrary process outside the App Store, so this is the only choice rather
//! than the best one. The risk has to be written down: if Apple removes it, this backend
//! dies outright with no way to patch around it — [`Seatbelt::detect`] returns `None`, the
//! provider falls back to [`crate::Unconfined`], and the approval dialog starts saying
//! confinement is unavailable. That is the correct behaviour for that day.
//!
//! The profile talks only about **file writes**. No `(deny network*)`: it genuinely blocks,
//! but it breaks `cargo`, `npm`, `pip` and `git` — that is, every command people actually
//! run — so turning it on only leads users to switch to `danger-full-access` and be done.
//! No `(deny file-read*)` either: a coding agent has to read the toolchain and the caches.
//!
//! One small, fatal detail: Seatbelt matches on **resolved paths**. `/tmp` is
//! `/private/tmp`, `/var/folders/...` is `/private/var/folders/...`. Forget to canonicalise
//! and the profile is still valid, `sandbox-exec` still runs, and the allowed area points at
//! somewhere nobody touches. [`crate::writable_roots`] canonicalises already; do not slip
//! raw paths through here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// An absolute path, never a `PATH` lookup: a `sandbox-exec` somebody else placed earlier
/// on `PATH` is a sandbox somebody else wrote.
pub const RUNNER: &str = "/usr/bin/sandbox-exec";

/// The stderr line macOS prints when the profile **refuses** a write.
///
/// This is evidence the sandbox is doing its job, not evidence the command is broken — and
/// telling those apart is the job of whoever consumes the result, so the dialect belongs
/// here rather than scattered across the tools.
pub const DENIAL_SIGNATURE: &str = "operation not permitted";

pub struct Seatbelt {
    runner: PathBuf,
}

impl Seatbelt {
    /// Whether `sandbox-exec` is usable on this machine.
    ///
    /// Probes by **actually running** an empty profile rather than checking the file
    /// exists: a Mac App Store build runs inside App Sandbox itself, and there
    /// `sandbox-exec` is present but cannot spawn. "The file is there" and "confinement
    /// works" are two different sentences, and only the second is worth reporting.
    pub fn detect() -> Option<Seatbelt> {
        let runner = PathBuf::from(RUNNER);
        if !runner.exists() {
            return None;
        }
        let probed = Command::new(&runner)
            .args(["-p", "(version 1)(allow default)", "--", "/usr/bin/true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match probed {
            Ok(status) if status.success() => Some(Seatbelt { runner }),
            _ => None,
        }
    }

    /// The test constructor: skips the probe.
    pub fn with_runner(runner: impl Into<PathBuf>) -> Seatbelt {
        Seatbelt {
            runner: runner.into(),
        }
    }

    pub fn runner(&self) -> &Path {
        &self.runner
    }
}

impl SandboxProvider for Seatbelt {
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError> {
        if argv.is_empty() {
            return Err(SandboxError::EmptyArgv);
        }
        // `danger-full-access` goes straight through, no runner: see
        // [`crate::Mode::confining`].
        if !policy.mode.confining() {
            return Ok(argv);
        }
        let mut wrapped = Vec::with_capacity(argv.len() + 4);
        wrapped.push(self.runner.display().to_string());
        wrapped.push("-p".to_string());
        wrapped.push(profile(policy));
        // `--` closes the option section. Without it, a command starting with `-` gets
        // swallowed by `sandbox-exec` as one of its own flags.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
    }

    fn enforcement(&self) -> Enforcement {
        // `Full` covers file effects **only**, and that is the entire scope of this
        // vocabulary: see the crate docs. The kernel enforces `deny file-write*` with no
        // exception the caller needs to know about.
        Enforcement::Full
    }

    /// True: `(deny network*)` is enforced by the kernel here, not by convention.
    fn network_confinable(&self) -> bool {
        true
    }
}

/// The SBPL profile for a policy.
///
/// Clause order is semantics, not aesthetics: SBPL takes the **last matching rule**, so
/// `(allow default)` has to come before `(deny file-write*)`, and every `allow` for a
/// writable area has to come after it. Reversed, the profile is still valid and confines
/// nothing.
pub fn profile(policy: &Policy) -> String {
    let mut forms = vec![
        "(version 1)".to_string(),
        "(allow default)".to_string(),
        "(deny file-write*)".to_string(),
        // The mandatory hole. Without it `read-only` cannot run a single command: nearly
        // everything opens `/dev/null` to discard output.
        format!("(allow file-write* (literal {}))", sbpl_string("/dev/null")),
    ];
    // Network. Last-matching-rule again: this has to come after `(allow default)`, and it
    // is emitted **only** when the caller asked for it. `(deny network*)` is a real kernel
    // control on macOS, which is why it can be reported as confinement rather than hope.
    if policy.deny_network {
        forms.push("(deny network*)".to_string());
    }
    let roots = writable_roots(policy);
    if !roots.is_empty() {
        let subpaths = roots
            .iter()
            .map(|root| format!("(subpath {})", sbpl_string(&root.display().to_string())))
            .collect::<Vec<_>>()
            .join(" ");
        forms.push(format!("(allow file-write* {subpaths})"));
    }
    forms.join("\n")
}

/// An SBPL string. Escape first, embed second.
///
/// An unescaped double quote in a path does not break the profile loudly — it **changes
/// what the profile means**, and `sandbox-exec` happily enforces the new meaning.
fn sbpl_string(raw: &str) -> String {
    let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
