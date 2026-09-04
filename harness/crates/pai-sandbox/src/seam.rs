//! The process-confinement seam. [`SandboxProvider::wrap`] rewrites argv, since every OS
//! confines by having a process bind itself then `exec`. [`SandboxProvider::enforcement`]
//! reports whether confinement is real on this machine, which the approval dialog reads.

use std::sync::Arc;

use pai_core::ServiceKey;

use crate::policy::Policy;

/// How real the confinement is right now; three states, since `Partial` must not be rounded either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enforcement {
    /// The kernel enforces exactly what was declared; a write outside the area fails.
    Full,
    /// A boundary that leaks in known places; a caller needing an absolute one must refuse.
    Partial(String),
    /// Nothing at all. The command will run with the user's full privileges.
    None(String),
}

impl Enforcement {
    /// Is there any boundary at all, including a leaky one.
    pub fn confines(&self) -> bool {
        !matches!(self, Enforcement::None(_))
    }

    /// Is the boundary airtight. For places that need an absolute boundary.
    pub fn is_full(&self) -> bool {
        matches!(self, Enforcement::Full)
    }

    /// Why it is not airtight. `None` when it is.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Enforcement::Full => Option::None,
            Enforcement::Partial(reason) | Enforcement::None(reason) => Some(reason.as_str()),
        }
    }

    /// One word, for logs and for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            Enforcement::Full => "full",
            Enforcement::Partial(_) => "partial",
            Enforcement::None(_) => "none",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Confinement is impossible, so nothing runs; never fall through to the original argv.
    #[error("không giam được tiến trình trên máy này: {0}")]
    Unavailable(String),
    /// Empty argv: wrapping it would produce a runnable command nobody intended to run.
    #[error("argv rỗng: không có gì để giam")]
    EmptyArgv,
    /// A root in the policy could not be resolved.
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(std::path::PathBuf, String),
}

/// The seam's implementation; `wrap` takes ownership of argv because it mostly prepends to it.
pub trait SandboxProvider: Send + Sync + 'static {
    /// Wrap argv so the process runs confined; `danger-full-access` returns argv unchanged.
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError>;

    /// Whether this mode is genuinely enforced on the running machine.
    fn enforcement(&self) -> Enforcement;

    /// Can this provider honour [`Policy::deny_network`]; false by default, and TCP only on Linux.
    fn network_confinable(&self) -> bool {
        false
    }
}

/// The seam. Exactly one provider per realm, chosen by operating system at mount time.
pub enum Sandbox {}

impl ServiceKey for Sandbox {
    type Api = dyn SandboxProvider;
    const NAME: &'static str = "sandbox";
}

/// Pick the provider for the running machine: select by OS first, probe capabilities second.
#[cfg(target_os = "macos")]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    match crate::seatbelt::Seatbelt::detect() {
        Some(seatbelt) => Arc::new(seatbelt),
        Option::None => Arc::new(crate::Unconfined::new(
            "không tìm thấy /usr/bin/sandbox-exec trên máy này",
        )),
    }
}

/// See the macOS version.
#[cfg(target_os = "linux")]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    Arc::new(crate::landlock::Landlock::detect())
}

/// See the macOS version.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    Arc::new(crate::Unconfined::new(crate::unconfined::WINDOWS_REASON))
}
