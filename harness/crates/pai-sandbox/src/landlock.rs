//! Linux: Landlock.
//!
//! Landlock giam **tiến trình gọi nó**. Không có API nào kiểu "chạy lệnh này trong hộp"
//! như `sandbox-exec`, nên `wrap` không thể chỉ thêm phần đầu vào argv rồi xong — nó
//! phải chèn một binary trung gian ([`pai-landlock-run`](../bin/pai-landlock-run.rs)) tự
//! trói mình rồi `exec` lệnh thật. Đó là khác biệt cấu trúc duy nhất so với bản macOS, và
//! nó là lý do crate này có hai target chứ không phải một.
//!
//! Hai điều Landlock **không** làm, và cả hai đều đúng với giới hạn đã ghi ở đầu crate:
//! nó không quản mạng ở những ABI hiện hành, và ở đây nó không quản việc đọc.
//!
//! ABI được **thương lượng lúc chạy**, không lúc biên dịch. `CompatLevel::BestEffort` hạ
//! xuống theo kernel đang chạy và `RulesetStatus` nói nó đã hạ tới đâu — đó là chỗ
//! [`Enforcement::Partial`] đến từ, chứ không phải từ một phép đoán theo `uname`.

use std::path::{Path, PathBuf};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// Tên binary trung gian. Nó nằm cạnh tệp thực thi chính vì cả hai ra từ cùng một bản
/// build và được đóng gói cùng nhau.
const RUNNER: &str = "pai-landlock-run";

pub struct Landlock {
    runner: PathBuf,
    abi: Option<i32>,
}

impl Landlock {
    /// Dò cả hai nửa: kernel có Landlock không, và binary trung gian có nằm cạnh ta không.
    ///
    /// Thiếu một trong hai là không giam được, và hai lý do đó khác nhau nên chúng ra hai
    /// câu khác nhau — người đọc hộp thoại duyệt cần biết là thiếu kernel hay thiếu tệp.
    pub fn detect() -> Landlock {
        Landlock {
            runner: runner_path().unwrap_or_default(),
            abi: probe_abi(),
        }
    }

    /// Bản dùng cho test: chỉ định binary trung gian, bỏ qua bước tìm.
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

/// Hỏi kernel nó hiểu Landlock tới ABI nào.
///
/// Gọi thẳng syscall thay vì dùng crate: câu hỏi này phải trả lời được **trên mọi hệ điều
/// hành** để `enforcement()` nói được lý do, kể cả khi crate `landlock` không được biên
/// dịch vào bản này.
#[cfg(target_os = "linux")]
fn probe_abi() -> Option<i32> {
    // `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)` trả về số ABI.
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
        // `danger-full-access` đi thẳng, không qua runner: xem [`crate::Mode::confining`].
        if !policy.mode.confining() {
            return Ok(argv);
        }
        if let Enforcement::None(reason) = self.enforcement() {
            // Không giam được thì **không chạy**, và người gọi phải nói ra. Trả lại argv
            // trần ở đây là lặng lẽ bỏ vòng vây đúng lúc người dùng tin là có.
            return Err(SandboxError::Unavailable(reason));
        }

        let mut wrapped = Vec::with_capacity(argv.len() + writable_roots(policy).len() * 2 + 2);
        wrapped.push(self.runner.display().to_string());
        for root in writable_roots(policy) {
            wrapped.push("--allow-write".to_string());
            wrapped.push(root.display().to_string());
        }
        // `--` đóng phần tuỳ chọn, cùng lý do như bản macOS: một lệnh bắt đầu bằng `-` sẽ
        // bị bộ phân tích tham số của chính runner nuốt mất.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
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
        // ABI 1 chưa có `refer` (đổi tên/liên kết cứng qua thư mục), ABI 2 chưa có cắt
        // ngắn tệp. Cả hai đều là đường ghi, nên nói ra thay vì báo `Full`.
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
