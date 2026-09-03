//! Đính kèm tệp vào tin nhắn: giao diện đưa đường dẫn, lõi nói có dùng được không.
//!
//! # Vì sao phép kiểm này không nằm ở giao diện
//!
//! Chỉ phía này nhìn được đĩa. Một phép so chuỗi `startsWith` trong TypeScript trông như
//! đủ, cho tới khi gặp một symlink, một junction của Windows, hay hai cách viết hoa của
//! cùng một thư mục — và lúc ấy nó nói "được" cho một tệp mà `read` sẽ từ chối.
//!
//! # Vì sao từ chối **trước khi gửi**
//!
//! `pai-fs` chỉ cấp quyền đọc trong đúng thư mục dự án (xem `FsPlugin::new` trong
//! `harness.rs`), nên một đường dẫn ngoài dự án vẫn chèn được vào tin nhắn và vẫn gửi đi
//! được — nó chỉ hỏng muộn hơn, giữa lượt trả lời, bằng câu lỗi của một tool. Cùng một tin
//! xấu, nhưng một bên còn sửa được bằng cách thả tệp khác, còn một bên đã tiêu một lượt
//! gọi mô hình để nói ra điều mà lệnh này biết trước lúc người dùng buông chuột.

use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::AppState;

/// Một đường dẫn được thả vào hoặc chọn ra, sau khi lõi đã nhìn vào đĩa.
#[derive(Serialize)]
pub struct Attachment {
    /// Nguyên văn đường dẫn giao diện đưa sang, và cũng là chuỗi sẽ chèn vào ô soạn tin khi
    /// `error` là `None`.
    ///
    /// Trả lại dạng người dùng thấy chứ không phải dạng đã phân giải: `canonicalize` trên
    /// Windows sinh ra tiền tố `\?\`, và một tin nhắn đầy `\?\` là một tin nhắn không ai
    /// đọc lại được. Tầng tool phân giải lại lúc đọc, nên không mất gì cả.
    pub path: String,
    /// `None` là dùng được. Chuỗi là **câu người dùng đọc được**, đã mang sẵn tên tệp.
    pub error: Option<String>,
}

/// Lọc một lô đường dẫn. Không lô nào bị bỏ vì một đường dẫn hỏng.
///
/// Trả lỗi cho **cả lô** đúng một trường hợp: chưa có dự án. Đó không phải chuyện của một
/// tệp nào cả — mọi tệp trên đĩa đều nằm ngoài một thư mục không tồn tại — nên nói nó một
/// lần là đúng, còn lặp lại cùng một câu cho từng tệp là bắt người dùng đọc năm lần.
#[tauri::command]
pub async fn resolve_attachments(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<Attachment>, String> {
    let harness = state.harness().await?;
    let workspace = harness
        .workspace()
        .ok_or_else(|| "Chưa mở dự án, nên chưa có thư mục nào để đính kèm tệp vào.".to_string())?;
    // Phân giải gốc **một lần**, và phân giải cả hai đầu: so một đường đã theo symlink với
    // một đường chưa theo là cách một tệp nằm trong dự án bị báo là nằm ngoài.
    let root = workspace
        .canonicalize()
        .map_err(|err| format!("Không đọc được thư mục dự án {}: {err}", workspace.display()))?;

    Ok(paths
        .into_iter()
        .map(|path| {
            let error = check(Path::new(&path), &root).err();
            Attachment { path, error }
        })
        .collect())
}

/// Tên hiện trong câu lỗi: tên tệp, không phải cả đường dẫn.
///
/// Đường dẫn tuyệt đối trong một câu lỗi giữa ô soạn tin đẩy phần đáng đọc — *vì sao* —
/// ra khỏi dòng. Người dùng vừa tự tay thả tệp ấy vào; họ nhận ra nó qua cái tên.
fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn check(path: &Path, root: &Path) -> Result<(), String> {
    let resolved = path
        .canonicalize()
        .map_err(|_| format!("Không tìm thấy {} trên đĩa.", name(path)))?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "{} nằm ngoài thư mục dự án, nên trợ lý không đọc được nó.",
            name(path)
        ));
    }
    Ok(())
}
