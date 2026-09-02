//! Hoàn thành `@` trong ô soạn tin: đường dẫn tệp của dự án đang mở.
//!
//! # Vì sao đi qua chỉ mục chứ không đi quét thư mục
//!
//! Ô soạn tin gọi lại sau **mỗi phím gõ**. Một lần đi cây thư mục cho mỗi phím là đọc lại
//! cả repo mười lần khi người ta gõ `handler`, và người dùng cảm thấy đúng chỗ họ ít chịu
//! đựng nhất. Chỉ mục đã có bảng `files` và đã bắt kịp đĩa theo `mtime` — hỏi nó là một
//! câu SQL trên một bảng nằm sẵn trong RAM.
//!
//! Hệ quả phải nói ra: gợi ý chỉ thấy **những tệp chỉ mục đã quét**. Một tệp vừa tạo ra
//! trong lần quét trước sẽ chưa có ở đây. Đó là đánh đổi đúng — chỉ mục tự bắt kịp ở lần
//! quét sau, còn cái giá của phương án kia là gõ bị khựng.
//!
//! # Ba loại dự án, ba câu trả lời
//!
//! Dự án mã nguồn trả về đường dẫn từ chỉ mục. Dự án tài liệu trả về đường dẫn tài liệu —
//! cùng một cử chỉ `@` phải làm được cùng một việc ở cả hai, nếu không người dùng học một
//! thói quen chỉ đúng một nửa thời gian. Chưa mở dự án thì **rỗng**, không phải lỗi: không
//! có dự án là một trạng thái hợp lệ, và một hộp thoại lỗi khi gõ `@` là phạt người dùng
//! vì đã thử.

use pai_index::Index;
use pai_rag::Docs;
use tauri::State;

use crate::AppState;

/// Trần cứng cho số gợi ý trả về.
///
/// Một danh sách dài hơn chỗ nhìn thấy không giúp chọn nhanh hơn; nó chỉ đẩy phần có ích
/// ra khỏi màn hình. Giao diện xin bao nhiêu cũng bị cắt về đây.
const MAX_HITS: usize = 20;

#[tauri::command]
pub async fn complete_paths(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<String>, String> {
    let harness = state.harness().await?;
    let limit = limit.clamp(1, MAX_HITS);

    if let Some(index) = harness.ctx.get::<Index>() {
        return index
            .paths(&query, limit)
            .await
            .map_err(|err| err.to_string());
    }

    if let Some(docs) = harness.ctx.get::<Docs>() {
        let paths: Vec<String> = docs
            .documents()
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|doc| doc.path.display().to_string())
            .collect();
        // Cùng bộ chấm điểm với dự án mã nguồn. Viết lại một bộ thứ hai ở đây là để hai
        // màn hình xếp hạng khác nhau cho cùng một truy vấn, và không ai biết cái nào đúng.
        return Ok(pai_index::complete::rank(&paths, &query, limit));
    }

    Ok(Vec::new())
}
