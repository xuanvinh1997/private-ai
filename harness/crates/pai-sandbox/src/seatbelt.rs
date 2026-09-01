//! macOS: `sandbox-exec` với hồ sơ SBPL sinh động.
//!
//! **Đứng trên nền đã bị Apple đánh dấu deprecated.** `sandbox-exec` mang cảnh báo
//! deprecated từ macOS 10.7 và `sandbox.h` cũng vậy; đến 2026 nó vẫn chạy, và chính App
//! Sandbox vẫn dựa trên cùng bộ máy. Không có API công khai nào thay thế được nó cho
//! việc giam một tiến trình tuỳ ý ngoài App Store, nên đây là lựa chọn duy nhất chứ
//! không phải lựa chọn tốt nhất. Rủi ro phải viết ra: nếu Apple gỡ nó, backend này chết
//! hẳn và không có đường vá — [`Seatbelt::detect`] sẽ trả `None`, provider rơi về
//! [`crate::Unconfined`], và hộp thoại duyệt bắt đầu nói "không giam được". Đó là hành
//! vi đúng cho ngày đó.
//!
//! Hồ sơ chỉ nói về **ghi tệp**. Không có `(deny network*)`: nó chặn được thật, nhưng nó
//! phá `cargo`, `npm`, `pip` và `git` — nghĩa là mọi lệnh người ta thật sự chạy — nên
//! bật nó lên chỉ dẫn tới việc người dùng chuyển sang `danger-full-access` cho xong.
//! Cũng không có `(deny file-read*)`: một coding agent phải đọc được toolchain và cache.
//!
//! Một chi tiết nhỏ và chết người: Seatbelt so khớp trên **đường dẫn đã phân giải**.
//! `/tmp` là `/private/tmp`, `/var/folders/...` là `/private/var/folders/...`. Quên
//! chuẩn hoá thì hồ sơ vẫn hợp lệ, `sandbox-exec` vẫn chạy, và vùng cho phép trỏ vào
//! một chỗ không ai chạm tới. [`crate::writable_roots`] chuẩn hoá sẵn; đừng luồn đường
//! dẫn thô qua đây.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::policy::{Policy, writable_roots};
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// Đường dẫn tuyệt đối, không tra `PATH`: một `sandbox-exec` do người khác đặt trước
/// trong `PATH` là một sandbox do người khác viết.
pub const RUNNER: &str = "/usr/bin/sandbox-exec";

/// Dòng stderr mà macOS in ra khi hồ sơ **từ chối** một thao tác ghi.
///
/// Đây là bằng chứng sandbox đang làm đúng việc, không phải bằng chứng lệnh hỏng — và
/// phân biệt hai thứ đó là việc của người tiêu thụ kết quả, nên dialect phải nằm ở đây
/// chứ không nằm rải rác trong tool.
pub const DENIAL_SIGNATURE: &str = "operation not permitted";

pub struct Seatbelt {
    runner: PathBuf,
}

impl Seatbelt {
    /// Có dùng được `sandbox-exec` trên máy này không.
    ///
    /// Dò bằng cách **chạy thật** một hồ sơ rỗng, không phải bằng cách kiểm tệp có tồn
    /// tại: bản đóng gói cho Mac App Store tự nó chạy trong App Sandbox và ở đó
    /// `sandbox-exec` có mặt nhưng không spawn được. "Tệp có đó" và "giam được" là hai
    /// câu khác nhau, và chỉ câu thứ hai đáng để báo cáo.
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

    /// Bản dùng cho test: bỏ qua bước dò.
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
        // `danger-full-access` đi thẳng, không qua runner: xem [`crate::Mode::confining`].
        if !policy.mode.confining() {
            return Ok(argv);
        }
        let mut wrapped = Vec::with_capacity(argv.len() + 4);
        wrapped.push(self.runner.display().to_string());
        wrapped.push("-p".to_string());
        wrapped.push(profile(policy));
        // `--` đóng phần tuỳ chọn. Không có nó, một lệnh bắt đầu bằng `-` sẽ bị
        // `sandbox-exec` nuốt mất làm cờ của chính nó.
        wrapped.push("--".to_string());
        wrapped.extend(argv);
        Ok(wrapped)
    }

    fn enforcement(&self) -> Enforcement {
        // `Full` **chỉ** cho hiệu ứng lên tệp, và đó là toàn bộ phạm vi của từ vựng này:
        // xem tài liệu crate. Kernel thi hành `deny file-write*` không có ngoại lệ nào
        // mà người gọi cần biết.
        Enforcement::Full
    }
}

/// Hồ sơ SBPL cho một chính sách.
///
/// Thứ tự các mệnh đề là ngữ nghĩa, không phải thẩm mỹ: SBPL lấy **luật khớp cuối cùng**,
/// nên `(allow default)` phải đứng trước `(deny file-write*)`, và mọi `allow` cho vùng
/// ghi được phải đứng sau nó. Đảo lại thì hồ sơ vẫn hợp lệ và không giam gì cả.
pub fn profile(policy: &Policy) -> String {
    let mut forms = vec![
        "(version 1)".to_string(),
        "(allow default)".to_string(),
        "(deny file-write*)".to_string(),
        // Cống bắt buộc. Không có nó thì `read-only` không chạy nổi một lệnh nào:
        // gần như mọi thứ đều mở `/dev/null` để vứt output đi.
        format!("(allow file-write* (literal {}))", sbpl_string("/dev/null")),
    ];
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

/// Chuỗi SBPL. Escape trước, nhúng sau.
///
/// Một dấu nháy kép chưa escape trong đường dẫn không làm hồ sơ hỏng một cách ồn ào — nó
/// làm hồ sơ **đổi nghĩa**, và `sandbox-exec` sẽ vui vẻ thi hành cái nghĩa mới.
fn sbpl_string(raw: &str) -> String {
    let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
