//! Đường dẫn: chuẩn hoá, rồi mới kiểm tra.
//!
//! Thứ tự đó là toàn bộ nội dung của tệp này. Kiểm tra trước khi chuẩn hoá nghĩa là
//! `gốc/../../etc/passwd` đi qua được, vì lúc so khớp nó vẫn còn bắt đầu bằng `gốc/`.
//!
//! Có hai cách chuẩn hoá, và cần cả hai. Đường **đọc** dùng `canonicalize` của hệ điều
//! hành — nó theo symlink, đúng thứ ta muốn: một symlink trỏ ra ngoài gốc phải bị coi là
//! nằm ngoài gốc. Đường **ghi** không dùng được nó, vì tệp chưa tồn tại; nên ta giải
//! `..` trên chuỗi rồi `canonicalize` tổ tiên gần nhất đang tồn tại, và ghép phần đuôi
//! vào. Phần đã tồn tại vẫn được kiểm qua symlink, phần chưa tồn tại thì không có
//! symlink nào để mà theo.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("{0} nằm ngoài những thư mục ứng dụng được phép đọc")]
    OutsideRoots(PathBuf),
    #[error("{0} là tệp được bảo vệ và không bao giờ đọc được")]
    Protected(PathBuf),
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(PathBuf, String),
}

/// Giải `.` và `..` trên chuỗi, không chạm đĩa.
///
/// Không dùng được `canonicalize` cho việc này vì nó đòi tệp phải tồn tại. Cũng không
/// bỏ qua được: `..` chưa giải là cách một đường dẫn thoát ra khỏi gốc mà vẫn trông như
/// đang ở trong.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                // `..` ở đầu một đường dẫn tương đối không có gì để bỏ; giữ nguyên rồi
                // để tầng gốc từ chối, thay vì lặng lẽ nuốt mất nó.
                if !out.pop() {
                    out.push(Component::ParentDir);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Những thư mục ứng dụng được phép chạm, và những tệp không bao giờ được chạm.
#[derive(Debug, Default, Clone)]
pub struct FileRoots {
    roots: Vec<PathBuf>,
    protected: Vec<PathBuf>,
}

impl FileRoots {
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
    ) -> FileRoots {
        FileRoots {
            roots: roots.into_iter().map(|p| lexical(&p)).collect(),
            protected: protected.into_iter().map(|p| lexical(&p)).collect(),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Khớp tuyệt đối, không theo tiền tố: cái được bảo vệ là *tệp đó*, không phải cả
    /// cây dưới nó.
    pub fn is_protected(&self, resolved: &Path) -> bool {
        self.protected.iter().any(|p| p == resolved)
    }

    fn within_roots(&self, resolved: &Path) -> bool {
        // Không có gốc nào nghĩa là chưa ai cấp quyền gì — từ chối tất, chứ không phải
        // cho phép tất. Cấu hình trống là cấu hình chặt nhất.
        self.roots.iter().any(|root| resolved.starts_with(root))
    }

    /// Phân giải một đường dẫn để **đọc**. Theo symlink.
    pub fn resolve_read(&self, path: &Path) -> Result<PathBuf, PathError> {
        let resolved = path
            .canonicalize()
            .map_err(|err| PathError::Unresolvable(path.to_path_buf(), err.to_string()))?;
        self.authorize(resolved)
    }

    /// Phân giải một đường dẫn để **ghi**. Tệp chưa cần tồn tại; thư mục cha thì cần.
    pub fn resolve_write(&self, path: &Path) -> Result<PathBuf, PathError> {
        let lexical = lexical(path);
        if let Ok(resolved) = lexical.canonicalize() {
            return self.authorize(resolved);
        }
        // Leo lên tới tổ tiên gần nhất đang tồn tại, phân giải nó, rồi ghép đuôi lại.
        let mut ancestors = lexical.ancestors().skip(1);
        let existing = ancestors
            .find(|a| a.exists())
            .ok_or_else(|| PathError::OutsideRoots(lexical.clone()))?;
        let base = existing
            .canonicalize()
            .map_err(|err| PathError::Unresolvable(existing.to_path_buf(), err.to_string()))?;
        let tail = lexical
            .strip_prefix(existing)
            .map_err(|_| PathError::OutsideRoots(lexical.clone()))?;
        self.authorize(base.join(tail))
    }

    fn authorize(&self, resolved: PathBuf) -> Result<PathBuf, PathError> {
        // Được bảo vệ được hỏi **trước**: một tệp vừa nằm trong gốc vừa được bảo vệ thì
        // câu trả lời là không, và lý do phải là lý do đúng.
        if self.is_protected(&resolved) {
            return Err(PathError::Protected(resolved));
        }
        if !self.within_roots(&resolved) {
            return Err(PathError::OutsideRoots(resolved));
        }
        Ok(resolved)
    }
}

/// Bốn nghìn byte đầu có byte không thì coi là nhị phân.
///
/// Từ chối hẳn thay vì trả về một chuỗi đầy ký tự thay thế: một tệp nhị phân đọc hỏng
/// trông y hệt một tệp văn bản mã hoá sai, và mô hình sẽ suy diễn trên rác.
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(4096).any(|byte| *byte == 0)
}
