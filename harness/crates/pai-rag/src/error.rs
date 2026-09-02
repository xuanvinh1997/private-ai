//! Lỗi của thư viện tài liệu.
//!
//! Mỗi nhánh ở đây tồn tại vì **người dùng phải đọc được nó và biết làm gì tiếp**. Một
//! `RagError::Other(String)` duy nhất thì rẻ để viết và vô dụng để đọc: người vừa kéo hai
//! mươi tệp vào cần phân biệt "tệp này quá to" với "bộ nhúng đang tắt" — cái đầu họ sửa
//! bằng cách bỏ tệp ra, cái sau bằng cách bật Ollama lên.
//!
//! Vì thế lỗi ở đây luôn nói ra **con số hoặc cái tên** gây ra nó, chứ không nuốt chúng
//! vào một câu chung chung.

/// Lỗi ở tầng tệp — nạp một tài liệu — và ở tầng kho.
#[derive(Debug, thiserror::Error)]
pub enum RagError {
    #[error("kho thư viện: {0}")]
    Store(String),

    #[error("không rút được chữ từ {path}: {reason}")]
    Extract { path: String, reason: String },

    /// Trần kích thước nói ra cả hai con số. "Tệp quá lớn" mà không có số là một câu
    /// buộc người dùng thử lại từng tệp một để đoán ngưỡng.
    #[error("{path} nặng {bytes} byte, vượt trần {limit} byte của thư viện tài liệu")]
    TooLarge {
        path: String,
        bytes: u64,
        limit: u64,
    },

    #[error("{0} trông như tệp nhị phân; thư viện chỉ nhận văn bản, PDF và DOCX")]
    Binary(String),

    /// Tệp mở ra được, bộ rút chữ chạy xong, và **không có lấy một ký tự nào**.
    ///
    /// Nhánh riêng chứ không gộp vào [`RagError::Extract`], vì nó là nhánh duy nhất mà
    /// mọi bước trước đó đều báo thành công. Không có nó, tài liệu được ghi vào kho với 0
    /// đoạn và `error` rỗng — mà "chưa có vector, chưa có lỗi" chính là định nghĩa của
    /// *đang xếp hàng* ở giao diện. Người dùng nhìn thấy một tài liệu chờ nhúng vĩnh
    /// viễn, trong khi thật ra không có gì để nhúng và sẽ không bao giờ có.
    #[error("{path} không có chữ nào để nạp: {reason}")]
    Empty { path: String, reason: String },

    #[error("chưa rút được chữ từ định dạng của {0}")]
    Unsupported(String),

    /// Nhúng hỏng **không** phải lỗi chí mạng của việc nạp — xem [`crate::library`]. Nó
    /// lên tới đây chỉ để được ghi vào hàng tài liệu rồi kể lại trong `stats()`.
    #[error("bộ nhúng `{id}` không dùng được: {reason}")]
    Embed { id: String, reason: String },

    #[error("không có tài liệu nào mang mã `{0}`")]
    NotFound(String),

    #[error("{path}: {reason}")]
    Io { path: String, reason: String },

    #[error("{0}")]
    Unavailable(String),
}

impl RagError {
    pub(crate) fn io(path: impl std::fmt::Display, err: impl std::fmt::Display) -> RagError {
        RagError::Io {
            path: path.to_string(),
            reason: err.to_string(),
        }
    }

    pub(crate) fn extract(
        path: impl std::fmt::Display,
        reason: impl std::fmt::Display,
    ) -> RagError {
        RagError::Extract {
            path: path.to_string(),
            reason: reason.to_string(),
        }
    }

    pub(crate) fn empty(path: impl std::fmt::Display, reason: impl std::fmt::Display) -> RagError {
        RagError::Empty {
            path: path.to_string(),
            reason: reason.to_string(),
        }
    }
}

impl From<rusqlite::Error> for RagError {
    fn from(err: rusqlite::Error) -> RagError {
        RagError::Store(err.to_string())
    }
}
