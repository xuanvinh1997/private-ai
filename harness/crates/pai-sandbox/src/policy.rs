//! Ba chế độ, và **một** định nghĩa duy nhất về "ghi được ở đâu".
//!
//! [`writable_roots`] là hàm duy nhất trả lời câu hỏi đó. Hồ sơ SBPL của macOS, ruleset
//! Landlock của Linux và hàng rào trong tiến trình của `pai-fs` đều phải hỏi nó, vì hai
//! nơi tự trả lời rồi lệch nhau chính là hình dạng của một lỗ hổng: người dùng thấy tool
//! `write` từ chối một đường dẫn, kết luận rằng ranh giới có thật, rồi một lệnh `bash`
//! ghi vào đúng đường dẫn ấy.
//!
//! Đường dẫn được **chuẩn hoá** trước khi vào danh sách. Đây không phải chuyện gọn gàng:
//! Seatbelt so khớp trên đường dẫn đã phân giải, nên `/tmp` — thực chất là
//! `/private/tmp` trên macOS — mà không chuẩn hoá thì cho phép một thư mục không ai
//! chạm tới, và vòng vây trông vẫn như đang mở đúng chỗ.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Chế độ giam. Tên trên dây giữ nguyên của dsh để cấu hình và sổ tay phiên dùng lại được.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Không ghi được gì, kể cả trong workspace. Chỉ còn những cống bắt buộc (`/dev/null`).
    ReadOnly,
    /// Ghi được trong workspace và trong thư mục tạm. Mọi chỗ khác bị từ chối.
    WorkspaceWrite,
    /// Không giam gì cả. Đây là *vắng mặt* của sandbox, không phải một cấu hình của nó.
    DangerFullAccess,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::ReadOnly => "read-only",
            Mode::WorkspaceWrite => "workspace-write",
            Mode::DangerFullAccess => "danger-full-access",
        }
    }

    /// Chế độ này có yêu cầu một vòng vây không.
    ///
    /// `danger-full-access` trả `false`, và đó là lý do nó không bao giờ đi qua một
    /// backend nào: bọc argv bằng một runner rồi cấp cho nó mọi quyền chỉ thêm một tiến
    /// trình vào cây và thêm một dialect lỗi phải đoán, đổi lại không được gì.
    pub fn confining(self) -> bool {
        !matches!(self, Mode::DangerFullAccess)
    }
}

/// Chính sách cho **một** lần chạy.
///
/// `workspace_root` là thư mục làm việc bất biến của phiên, không phải `cwd` của lệnh.
/// Lấy `cwd` làm gốc thì một lệnh chạy trong thư mục con sẽ bị giam chặt hơn chính sách
/// người dùng đã duyệt, và một lệnh chạy trong thư mục cha thì lỏng hơn.
#[derive(Debug, Clone)]
pub struct Policy {
    pub mode: Mode,
    pub workspace_root: PathBuf,
}

impl Policy {
    pub fn new(mode: Mode, workspace_root: impl Into<PathBuf>) -> Policy {
        Policy {
            mode,
            workspace_root: workspace_root.into(),
        }
    }

    pub fn read_only(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::ReadOnly, workspace_root)
    }

    pub fn workspace_write(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::WorkspaceWrite, workspace_root)
    }

    pub fn danger_full_access(workspace_root: impl Into<PathBuf>) -> Policy {
        Policy::new(Mode::DangerFullAccess, workspace_root)
    }
}

/// Những gốc mà chế độ này cho phép ghi, đã chuẩn hoá và đã bỏ trùng.
///
/// Đường dẫn không tồn tại bị **bỏ đi** chứ không được giữ nguyên văn: không chuẩn hoá
/// được nghĩa là không biết nó thật sự trỏ vào đâu, và cấp quyền ghi cho một chuỗi chưa
/// phân giải là cấp quyền cho bất cứ thứ gì sau này chiếm được chỗ đó.
///
/// `danger-full-access` trả về danh sách rỗng, và đó không phải "không ghi được đâu cả"
/// mà là "câu hỏi này không dành cho chế độ đó" — người gọi phải kiểm [`Mode::confining`]
/// trước.
pub fn writable_roots(policy: &Policy) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if policy.mode == Mode::WorkspaceWrite {
        let temp = std::env::temp_dir();
        for candidate in [
            policy.workspace_root.clone(),
            temp,
            PathBuf::from("/tmp"),
            PathBuf::from("/var/tmp"),
        ] {
            if let Ok(resolved) = candidate.canonicalize() {
                roots.push(resolved);
            }
        }
    }
    dedup_nested(roots)
}

/// Bỏ bản trùng và bỏ những gốc đã nằm trong một gốc khác.
///
/// Không phải để cho danh sách ngắn: `/tmp` và `/private/tmp` chuẩn hoá về cùng một chỗ
/// trên macOS, và một hồ sơ SBPL liệt kê hai lần cùng một `subpath` là một hồ sơ khiến
/// người đọc nó tưởng có hai vùng khác nhau.
fn dedup_nested(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort();
    roots.dedup();
    let picked: Vec<PathBuf> = roots
        .iter()
        .filter(|root| {
            !roots
                .iter()
                .any(|other| other.as_path() != root.as_path() && root.starts_with(other))
        })
        .cloned()
        .collect();
    picked
}

/// Đường dẫn có nằm trong một gốc được phép ghi không.
///
/// Dành cho hàng rào trong tiến trình và cho test; backend ngoài tiến trình không dùng
/// nó, vì chúng giao việc so khớp cho kernel.
pub fn is_writable(policy: &Policy, path: &Path) -> bool {
    if policy.mode == Mode::DangerFullAccess {
        return true;
    }
    writable_roots(policy)
        .iter()
        .any(|root| path.starts_with(root))
}
