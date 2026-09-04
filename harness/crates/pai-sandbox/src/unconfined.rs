//! The provider for machines that cannot confine yet, today Windows. Not a stub: it answers
//! "no, for this reason", which the approval dialog needs to distinguish from silence.
//! A restricted token is the only viable Windows primitive, and it is not written yet.

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
        // `danger-full-access` still runs; the confining modes error rather than pass through.
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
