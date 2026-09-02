//! Linux: Landlock.
//!
//! Landlock confines **the process that calls it**. There is no "run this command in a box"
//! API like `sandbox-exec`, so `wrap` cannot simply prepend to argv and be done — it has to
//! insert a helper binary ([`pai-landlock-run`](../bin/pai-landlock-run.rs)) that binds
//! itself and then `exec`s the real command. That is the one structural difference from the
//! macOS version, and it is why this crate has two targets rather than one.
//!
//! What Landlock governs here, and what it does not. Network confinement is **opt-in** and
//! real from ABI 4 — handling TCP without adding a `NetPort` rule is a total block on bind
//! and connect — but it is **TCP only**, because Landlock has no UDP verb, so DNS and every
//! UDP transport still leave the box. Below ABI 4 the kernel has no network rules at all and
//! [`Landlock::network_confinable`] says so rather than accepting the flag and doing nothing.
//! Reads are not governed here, consistent with the limits stated at the top of the crate.
//!
//! The ABI is **negotiated at run time**, not compile time. `CompatLevel::BestEffort` steps
//! down to match the running kernel and `RulesetStatus` reports how far it stepped — that is
//! where [`Enforcement::Partial`] comes from, rather than from a guess based on `uname`.

use std::path::{Path, PathBuf};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// The helper binary's name. It sits next to the main executable because both come out of
/// the same build and are packaged together.
const RUNNER: &str = "pai-landlock-run";

pub struct Landlock {
    runner: PathBuf,
    abi: Option<i32>,
}

impl Landlock {
    /// Probe both halves: does the kernel have Landlock, and is the helper binary next to
    /// us.
    ///
    /// Missing either means no confinement, and the two reasons differ, so they produce two
    /// different sentences — whoever reads the approval dialog needs to know whether the
    /// kernel or the file is missing.
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

/// Ask the kernel which Landlock ABI it understands.
///
/// Calls the syscall directly rather than going through the crate: this question has to be
/// answerable **on every operating system** so `enforcement()` can state a reason, even when
/// the `landlock` crate is not compiled into this build.
#[cfg(target_os = "linux")]
fn probe_abi() -> Option<i32> {
    // `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` returns the ABI
    // number.
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
        // `danger-full-access` goes straight through, no runner: see
        // [`crate::Mode::confining`].
        if !policy.mode.confining() {
            return Ok(argv);
        }
        if let Enforcement::None(reason) = self.enforcement() {
            // No confinement means **nothing runs**, and the caller has to say so.
            // Returning bare argv here silently drops the boundary at exactly the moment
            // the user believes it is there.
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
        // `--` closes the option section, same reason as the macOS version: a command
        // starting with `-` would be swallowed by the runner's own argument parser.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
    }

    /// Only from ABI 4, and only for TCP.
    ///
    /// A kernel below 4 has no network verb at all, so saying yes there would hand the
    /// caller a boundary that the kernel never builds. And even above it the answer is
    /// narrower than the question sounds — Landlock governs TCP bind and connect, not UDP —
    /// which is why [`Policy::deny_network`] documents the gap rather than implying cover.
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
        // ABI 1 lacks `refer` (renames and hard links across directories); ABI 2 lacks
        // truncation. Both are write paths, so say so rather than reporting `Full`.
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
