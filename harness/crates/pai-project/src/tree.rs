//! Cây tệp và nội dung tệp, cho người đọc.
//!
//! Tách khỏi `pai-fs` là có chủ ý. `pai-fs` phục vụ **mô hình**: nó đánh số dòng, chèn
//! cảnh báo nội dung không đáng tin, và mọi lời gọi đi qua đường ống có canh gác. Chỗ này
//! phục vụ **người dùng** đang bấm vào một cái cây — không có mô hình nào trong vòng lặp,
//! nên không có gì để cảnh báo và không có gì để canh gác ngoài ranh giới thư mục.
//!
//! Trộn hai đường lại thì mỗi lần người dùng mở một tệp là một lần đi qua bộ máy dựng cho
//! một người đọc khác, và những quyết định đúng cho mô hình (cắt 2000 dòng, đóng khung
//! cảnh báo) trở thành sai cho giao diện.

use std::path::Path;

use ignore::WalkBuilder;
use serde::Serialize;

use crate::store::{ProjectError, canonical};

type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    /// Chỉ có khi đã nạp cấp đó. Vắng nghĩa là "chưa nạp", khác hẳn mảng rỗng nghĩa là
    /// "thư mục trống" — giao diện vẽ hai thứ đó khác nhau.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<TreeEntry>>,
}

/// Đọc cây, mặc định **một cấp**.
///
/// Nạp lười theo từng cấp chứ không quét cả repo: một cây đầy đủ của một repo lớn là hàng
/// chục nghìn hàng mà người dùng nhìn thấy chừng ba mươi, và thời gian chờ đó rơi đúng
/// vào lúc họ vừa bấm mở dự án.
pub fn list_tree(root: &Path, at: Option<&Path>, depth: usize) -> Result<Vec<TreeEntry>> {
    let root = canonical(root)?;
    let start = match at {
        Some(path) => {
            let resolved = canonical(path)?;
            if !resolved.starts_with(&root) {
                return Err(ProjectError::NotADirectory(resolved));
            }
            resolved
        }
        None => root.clone(),
    };
    Ok(walk(&start, depth.max(1)))
}

fn walk(dir: &Path, depth: usize) -> Vec<TreeEntry> {
    let mut entries: Vec<TreeEntry> = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        // `.gitignore` là tuyên bố của chính dự án về thứ không đáng nhìn. Bỏ qua nó thì
        // `node_modules` và `target` nuốt hết cây.
        .git_ignore(true)
        .git_global(true)
        // Không có `.git` thì `ignore` mặc định **bỏ qua** mọi `.gitignore`. Một dự án
        // chưa `git init` vẫn có `.gitignore` nói đúng thứ không đáng nhìn, và không có
        // dòng này thì `node_modules` nuốt hết cây ngay lần mở đầu tiên.
        .require_git(false)
        .build()
        .flatten()
        // Phần tử đầu của `WalkBuilder` là chính `dir`.
        .filter(|entry| entry.path() != dir)
        .map(|entry| {
            let path = entry.path().to_path_buf();
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            TreeEntry {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                children: (is_dir && depth > 1).then(|| walk(&path, depth - 1)),
                path: path.display().to_string(),
                is_dir,
            }
        })
        .collect();

    // Thư mục trước, rồi theo tên. Đây là thứ tự mọi trình duyệt tệp dùng, và một cây sắp
    // khác đi buộc người ta phải đọc thay vì quét mắt.
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    entries
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub text: String,
    /// Đuôi tệp. Việc đoán ngôn ngữ từ nó là của giao diện.
    pub lang: Option<String>,
    pub total_lines: u32,
    pub truncated: bool,
}

/// Trần số dòng cho một lần xem.
///
/// Cao hơn nhiều so với trần của `read` dành cho mô hình (2000): ở đó trần bảo vệ cửa sổ
/// ngữ cảnh, ở đây nó chỉ bảo vệ khỏi việc dựng một triệu phần tử DOM.
const MAX_LINES: usize = 20_000;

pub fn read_file(root: &Path, path: &Path) -> Result<FileView> {
    let root = canonical(root)?;
    let resolved = path
        .canonicalize()
        .map_err(|err| ProjectError::Unresolvable(path.to_path_buf(), err.to_string()))?;
    // Ranh giới duy nhất ở đây: không ra khỏi dự án. Chuẩn hoá trước rồi mới so, cùng luật
    // như `pai-fs` — so trước khi chuẩn hoá thì `..` đi lọt.
    if !resolved.starts_with(&root) {
        return Err(ProjectError::NotADirectory(resolved));
    }

    let bytes = std::fs::read(&resolved)
        .map_err(|err| ProjectError::Unresolvable(resolved.clone(), err.to_string()))?;
    if bytes.iter().take(4096).any(|byte| *byte == 0) {
        return Err(ProjectError::Unresolvable(resolved, "tệp nhị phân".into()));
    }

    let text = String::from_utf8_lossy(&bytes);
    let total = text.lines().count();
    let truncated = total > MAX_LINES;
    let shown = if truncated {
        text.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n")
    } else {
        text.into_owned()
    };

    Ok(FileView {
        lang: resolved
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned()),
        text: shown,
        total_lines: total as u32,
        truncated,
    })
}
