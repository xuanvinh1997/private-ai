//! What can go wrong, split along the line the model cares about: arguments it can fix
//! versus a machine it cannot. That split is what [`From<GitError> for ToolError`] encodes —
//! `Invalid` invites a retry with different arguments, `Failed` does not.

use std::path::PathBuf;

use pai_tools::ToolError;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("không chạy được `git`: {0} — hãy kiểm tra `git` đã có trong PATH chưa")]
    Missing(String),
    #[error("{0} không phải là một kho git — không tìm thấy `.git` ở đó hay ở thư mục cha nào")]
    NotARepo(PathBuf),
    #[error("không mở được thư mục gốc của dự án: {0} không còn tồn tại hoặc đã bị đổi tên")]
    RootGone(PathBuf),
    #[error("`git {command}` thất bại ({code}){detail}")]
    Command {
        command: String,
        code: String,
        /// Đã có dấu `: ` ở đầu khi khác rỗng, để câu trên vẫn đọc trôi khi git im lặng.
        detail: String,
    },
    #[error("không đọc được đầu ra của `git`: {0}")]
    Io(String),
    #[error("lệnh `git` bị huỷ trước khi chạy xong")]
    Cancelled,

    // --- Dưới đây là lỗi tham số: mô hình sửa được, nên chúng thành `ToolError::Invalid`. ---
    #[error("`{0}` rỗng — hãy bỏ hẳn tham số thay vì truyền chuỗi rỗng")]
    Empty(&'static str),
    #[error(
        "từ chối `{0}`: chuỗi bắt đầu bằng `-` sẽ bị `git` hiểu là một tuỳ chọn dòng lệnh chứ \
         không phải tên tệp hay tên nhánh"
    )]
    LeadingDash(String),
    #[error("`{0}` chứa ký tự điều khiển hoặc xuống dòng, không dùng làm tham số cho `git` được")]
    ControlChar(String),
    #[error(
        "từ chối pathspec `{0}`: dấu `:` mở đầu là cú pháp pathspec ma thuật của git, hãy \
         truyền một đường dẫn thường"
    )]
    MagicPathspec(String),
    #[error("`{0}` nằm ngoài kho git {1} — chỉ đọc được những gì nằm trong kho")]
    OutsideRepo(String, PathBuf),
}

impl GitError {
    /// Whether the model can fix this by calling again with different arguments.
    fn is_argument_fault(&self) -> bool {
        matches!(
            self,
            GitError::Empty(_)
                | GitError::LeadingDash(_)
                | GitError::ControlChar(_)
                | GitError::MagicPathspec(_)
                | GitError::OutsideRepo(..)
        )
    }
}

impl From<GitError> for ToolError {
    fn from(err: GitError) -> ToolError {
        if err.is_argument_fault() {
            ToolError::Invalid(err.to_string())
        } else {
            ToolError::Failed(err.to_string())
        }
    }
}
