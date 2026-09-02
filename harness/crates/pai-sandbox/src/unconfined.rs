//! The provider for machines that cannot confine yet — today, Windows.
//!
//! This is **not** a stub. It does not wrap argv, does not generate a profile, and never
//! returns `Enforcement::Full`. The only thing it does is answer "can this be confined"
//! with "no, for this reason" — and that answer has value, because in silence the approval
//! dialog cannot tell "nobody mounted a sandbox" apart from "one is mounted and it says no".
//!
//! Why Windows is missing: four primitives were surveyed and only **one** is viable.
//!
//! - **Restricted token** (`CreateRestrictedToken` with `WRITE_RESTRICTED` + a synthesised
//!   SID for the workspace + a Job Object) — viable, needs no administrator rights, and is
//!   what both dsh and the Codex CLI chose. But it only intersects with **write** access:
//!   reads, the network and the ability to see other processes are all unrestricted,
//!   `Everyone` must remain in the restricting list (removing it kills DLL init with
//!   `0xC0000142`) so every NTFS object granting write to `Everyone` is still writable, and
//!   NTFS hard links alias one file object across several paths. Meaning that when it does
//!   land, it has to report `Partial`, never `Full`.
//! - **AppContainer** — denies *reads* by default. A coding agent has to read the repo, the
//!   toolchain, the git config and the dependency cache; punching enough holes to make it
//!   work leaves the boundary meaningless. Capabilities must also be declared up front,
//!   while the agent picks binaries at run time.
//! - **Windows Sandbox (Hyper-V)** — absent from Home editions, and more importantly it
//!   cannot act on the user's real workspace. Right for a computer-use agent, wrong for a
//!   coding agent.
//! - **Mandatory Integrity Control (Low IL)** — leaves SACL labels on disk and affects every
//!   other Low-integrity process on the machine. It cannot carve out a boundary for the
//!   agent alone.
//!
//! So Windows sits at v1.0 on the roadmap, and until then the honest answer is the one in
//! this file.

use crate::policy::Policy;
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// The default reason for Windows, written once so every place says the same sentence.
pub const WINDOWS_REASON: &str = "Windows chưa có backend giam tiến trình: restricted \
     token là đường khả thi duy nhất và nó chưa được viết. Lệnh chạy với đầy đủ quyền \
     của bạn, và thứ duy nhất đứng giữa là hộp thoại duyệt.";

pub struct Unconfined {
    reason: String,
}

impl Unconfined {
    pub fn new(reason: impl Into<String>) -> Unconfined {
        Unconfined {
            reason: reason.into(),
        }
    }
}

impl SandboxProvider for Unconfined {
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError> {
        // `danger-full-access` asks for nothing, so it still runs here. The other two are
        // an **error**, not a passthrough: returning the original argv to a caller that
        // asked to be confined is exactly the behaviour that makes a non-existent boundary
        // look present.
        if argv.is_empty() {
            return Err(SandboxError::EmptyArgv);
        }
        if policy.mode.confining() {
            return Err(SandboxError::Unavailable(self.reason.clone()));
        }
        Ok(argv)
    }

    fn enforcement(&self) -> Enforcement {
        Enforcement::None(self.reason.clone())
    }
}
