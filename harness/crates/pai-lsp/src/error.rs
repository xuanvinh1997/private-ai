//! Every way an LSP query fails, plus the case where it has simply not finished yet.
//! Each variant exists because the model must do something different with it: retry soon
//! ([`LspError::NotReady`]), retry for another reason ([`LspError::Dead`]), or switch tool.

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
