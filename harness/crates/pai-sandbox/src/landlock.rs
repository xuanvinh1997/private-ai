//! Linux: Landlock confines the calling process, so `wrap` inserts a helper binary that binds
//! itself and then `exec`s the real command. Network confinement is opt-in, TCP only, and only
//! from ABI 4; the ABI is negotiated at run time, which is where `Partial` comes from.

use std::path::{Path, PathBuf};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// The helper binary's name; it sits next to the main executable, packaged with it.
const RUNNER: &str = "pai-landlock-run";

pub struct Landlock {
    runner: PathBuf,
    abi: Option<i32>,
}

impl Landlock {
    /// Probe both halves, kernel support and helper binary, since each missing half reads differently.
    pub fn detect() -> Landlock {
        Landlock {
            runner: runner_path().unwrap_or_default(),
            abi: probe_abi(),
        }
    }

    /// The test constructor: name the helper binary, skip the search.
    pub fn with_runner(runner: impl Into<PathBuf>) -> Landlock {
        Landlock {
            runner: runner.into(),
            abi: probe_abi(),
        }
    }

    pub fn runner(&self) -> &Path {
        &self.runner
    }
}

/// Ask the kernel for its Landlock ABI, by syscall, so the answer exists without the crate.
#[cfg(target_os = "linux")]
fn probe_abi() -> Option<i32> {
    // `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` returns the ABI number.
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const LANDLOCK_CREATE_RULESET_VERSION: libc::c_ulong = 1;
    let version = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<libc::c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    (version > 0).then_some(version as i32)
}

#[cfg(not(target_os = "linux"))]
fn probe_abi() -> Option<i32> {
    None
}

fn runner_path() -> Option<PathBuf> {
    let here = std::env::current_exe().ok()?;
    let candidate = here.parent()?.join(RUNNER);
    candidate.is_file().then_some(candidate)
}

impl SandboxProvider for Landlock {
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError> {
        if argv.is_empty() {
            return Err(SandboxError::EmptyArgv);
        }
        // `danger-full-access` goes straight through, no runner: see `crate::Mode::confining`.
        if !policy.mode.confining() {
            return Ok(argv);
        }
        if let Enforcement::None(reason) = self.enforcement() {
            // No confinement means nothing runs; bare argv here would drop the boundary silently.
            return Err(SandboxError::Unavailable(reason));
        }

        let mut wrapped = Vec::with_capacity(argv.len() + writable_roots(policy).len() * 2 + 3);
        wrapped.push(self.runner.display().to_string());
        if policy.deny_network {
            wrapped.push("--deny-network".to_string());
        }
        for root in writable_roots(policy) {
            wrapped.push("--allow-write".to_string());
            wrapped.push(root.display().to_string());
        }
        // `--` closes the options, or a command starting with `-` is eaten by the runner.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
    }

    /// Only from ABI 4, and only TCP bind and connect: Landlock has no UDP verb.
    fn network_confinable(&self) -> bool {
        self.abi.is_some_and(|abi| abi >= 4)
    }

    fn enforcement(&self) -> Enforcement {
        let Some(abi) = self.abi else {
            return Enforcement::None(
                "kernel này không có Landlock (cần Linux 5.13 trở lên, và LSM phải được bật)"
                    .to_string(),
            );
        };
        if !self.runner.is_file() {
            return Enforcement::None(format!(
                "không tìm thấy `{RUNNER}` cạnh tệp thực thi; bản cài đặt thiếu tệp"
            ));
        }
        // ABI 1 lacks `refer` and ABI 2 lacks truncation, both write paths, so never `Full`.
        match abi {
            1 => Enforcement::Partial(
                "Landlock ABI 1: chưa chặn được đổi tên và liên kết cứng qua thư mục".to_string(),
            ),
            2 => {
                Enforcement::Partial("Landlock ABI 2: chưa chặn được lệnh cắt ngắn tệp".to_string())
            }
            _ => Enforcement::Full,
        }
    }
}
