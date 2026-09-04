//! macOS: `sandbox-exec` with a generated SBPL profile, deprecated since 10.7 but still the
//! only way to confine an arbitrary process. The profile governs file writes only, and
//! Seatbelt matches resolved paths, so never let an uncanonicalised path through here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// An absolute path, never a `PATH` lookup: another `sandbox-exec` is another sandbox.
pub const RUNNER: &str = "/usr/bin/sandbox-exec";

/// The stderr line macOS prints when the profile refuses a write: the sandbox working, not a break.
pub const DENIAL_SIGNATURE: &str = "operation not permitted";

pub struct Seatbelt {
    runner: PathBuf,
}

impl Seatbelt {
    /// Is `sandbox-exec` usable here; probed by running it, since presence is not usability.
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
        // `danger-full-access` goes straight through, no runner: see `crate::Mode::confining`.
        if !policy.mode.confining() {
            return Ok(argv);
        }
        let mut wrapped = Vec::with_capacity(argv.len() + 4);
        wrapped.push(self.runner.display().to_string());
        wrapped.push("-p".to_string());
        wrapped.push(profile(policy));
        // `--` closes the options, or a command starting with `-` becomes a `sandbox-exec` flag.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
    }

    fn enforcement(&self) -> Enforcement {
        // `Full` covers file effects only, which is the whole scope of this vocabulary.
        Enforcement::Full
    }

    /// True: `(deny network*)` is enforced by the kernel here, not by convention.
    fn network_confinable(&self) -> bool {
        true
    }
}

/// The SBPL profile for a policy; clause order is semantics, as SBPL takes the last matching rule.
pub fn profile(policy: &Policy) -> String {
    let mut forms = vec![
        "(version 1)".to_string(),
        "(allow default)".to_string(),
        "(deny file-write*)".to_string(),
        // The mandatory hole: nearly every command opens `/dev/null` to discard output.
        format!("(allow file-write* (literal {}))", sbpl_string("/dev/null")),
    ];
    // Network: after `(allow default)` for last-matching-rule, and only when asked for.
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

/// An SBPL string, escaped first: an unescaped quote silently changes what the profile means.
fn sbpl_string(raw: &str) -> String {
    let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
