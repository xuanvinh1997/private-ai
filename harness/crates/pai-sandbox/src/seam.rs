//! The process-confinement seam.
//!
//! The interface has two functions, and the second is the important one.
//!
//! [`SandboxProvider::wrap`] wraps argv: it takes the **real** argv about to be spawned
//! (not a shell string) and returns a new one. The caller runs what came back, not what it
//! passed in. This is the only shape that works on all three operating systems, because all
//! three confine by having a process bind itself and then `exec` — no API can confine a
//! process that is already running.
//!
//! [`SandboxProvider::enforcement`] answers "on **this running machine**, is the
//! confinement real". It does not consult the policy, because the question does not belong
//! to the policy: `workspace-write` on a machine without Landlock is still
//! `workspace-write`, it is simply that nobody enforces it. This is what the approval
//! dialog must read, and it is why [`Enforcement`] has three states rather than two.

use std::sync::Arc;

use pai_core::ServiceKey;

use crate::policy::Policy;

/// How real the confinement is, on this machine, right now.
///
/// Three states rather than a `bool`, because `Partial` is the most common state in
/// practice and also the easiest to round away — rounding it up to "confined" is a lie, and
/// rounding it down to "unconfined" throws away a real layer of defence.
///
/// The reason attached to `Partial` and `None` is not log decoration: it is the sentence
/// the user reads in the approval dialog, immediately before clicking "allow".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enforcement {
    /// The kernel enforces exactly what was declared. A write outside the allowed area
    /// fails, rather than "usually fails".
    Full,
    /// There is a boundary, but it leaks in known places. A caller that needs an absolute
    /// boundary has to refuse or say so, never treat this as `Full`.
    Partial(String),
    /// Nothing at all. The command will run with the user's full privileges.
    None(String),
}

impl Enforcement {
    /// Is there any boundary at all — including a leaky one.
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
    /// Confinement is impossible, so **nothing runs**. The caller has to say so and must
    /// never quietly run the original argv: one silent fallthrough is one user trusting a
    /// boundary that does not exist.
    #[error("không giam được tiến trình trên máy này: {0}")]
    Unavailable(String),
    /// Empty argv. Not the sandbox's fault, but wrapping an empty argv produces a
    /// runnable command line that nobody intended to run.
    #[error("argv rỗng: không có gì để giam")]
    EmptyArgv,
    /// A root in the policy could not be resolved.
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(std::path::PathBuf, String),
}

/// The seam's implementation.
///
/// `wrap` takes `Vec<String>` rather than `&[String]` because in two of the three cases it
/// only prepends to that same argv — taking ownership means no copy is made only to be
/// thrown away.
pub trait SandboxProvider: Send + Sync + 'static {
    /// Wrap argv so the process runs confined. Returns the new argv.
    ///
    /// Under `danger-full-access` the implementation returns **exactly** the argv it was
    /// given: that mode is the absence of a sandbox, so wrapping it would build an empty
    /// boundary that then has to be maintained.
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError>;

    /// Whether this mode is genuinely enforced on the running machine.
    fn enforcement(&self) -> Enforcement;

    /// Whether this provider can actually honour [`Policy::deny_network`].
    ///
    /// Defaults to **false**, and the default is the whole point. A provider that cannot cut
    /// the network has to say so, because the alternative is a caller setting `deny_network`,
    /// getting no error, and believing in a boundary that was never built. That is the same
    /// rule as [`Enforcement`] itself: reported truth, never a promise.
    ///
    /// macOS says true — `(deny network*)` is a real kernel control there. Linux says true
    /// from Landlock ABI 4 and false below it, decided by asking the running kernel rather
    /// than by reading `uname`. Note what "true" buys on Linux: TCP bind and connect only.
    /// Landlock has no UDP verb, so DNS still leaves the box — which is why this is one bool
    /// per provider and not a single cross-platform promise.
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

/// Pick the provider for the running machine.
///
/// Select by operating system **first**, probe for capabilities **second**. The reverse
/// order sounds more general but probes backends that can never be present, and every probe
/// is a process spawn at startup.
///
/// Three implementations, one per operating system, rather than one function with three
/// `cfg` branches: a `cfg` branch inside a function body is compiled in exactly one place
/// and rots in the other two.
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
