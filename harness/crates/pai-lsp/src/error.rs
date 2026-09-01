//! Mọi cách một truy vấn LSP hỏng, và cả cách nó *chưa* hỏng mà chỉ chưa xong.
//!
//! Danh sách này ngắn có chủ ý, và mỗi nhánh tồn tại vì mô hình phải làm một việc **khác
//! nhau** với nó:
//!
//! - [`LspError::NotReady`] — thử lại sau vài giây thì được. Đây là nhánh quan trọng nhất
//!   của cả crate: `rust-analyzer` mất hàng chục giây để nạp một workspace, và gộp nó vào
//!   "không tìm thấy gì" là dạy mô hình rằng câu trả lời đúng là "hàm đó không tồn tại".
//! - [`LspError::Dead`] — server đã chết. Thử lại có ích, nhưng vì lý do khác hẳn.
//! - [`LspError::NoServer`] — không có server cho ngôn ngữ này; thử lại vô ích, đổi công
//!   cụ mới có ích. `symbol_search` của `pai-index` là câu trả lời, và thông báo nói ra.

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error(
        "không có language server nào cho `{0}`. Dùng `symbol_search` để tìm nơi khai báo \
         theo tên, hoặc `grep` để tìm mọi chỗ nhắc tới nó."
    )]
    NoServer(String),

    #[error(
        "language server `{0}` đã nhận việc nhưng chưa khởi động xong sau {1:?}. Nó đang \
         nạp và lập chỉ mục dự án; hãy hỏi lại sau vài giây, hoặc dùng `symbol_search` \
         nếu chỉ cần nơi khai báo."
    )]
    NotReady(String, Duration),

    #[error("language server `{0}` đã dừng: {1}")]
    Dead(String, String),

    #[error("language server `{0}` không trả lời trong {1:?}")]
    Timeout(String, Duration),

    #[error("không khởi động được language server `{0}`: {1}")]
    Launch(String, String),

    #[error("language server báo lỗi: {0}")]
    Protocol(String),

    #[error("{0}")]
    Invalid(String),
}
