//! Lỗi của sổ tay phiên.
//!
//! Mọi biến thể ở đây là một bất biến bị vi phạm, không phải một sự cố kỹ thuật. Chúng
//! được tách nhỏ vì người gọi phải phân biệt được: "sổ này hỏng, đừng ghi tiếp" khác hẳn
//! "yêu cầu của bạn sai, sửa rồi gọi lại".

use crate::event::Seq;

pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("không tìm thấy phiên `{0}`")]
    NotFound(String),

    #[error("phiên `{0}` đã tồn tại")]
    AlreadyExists(String),

    /// Ranh giới fork sai. Không bao giờ tự làm tròn: một ranh giới sai là một ý định
    /// sai, và làm tròn nó sẽ đẻ ra một phiên con mà người gọi không hề yêu cầu.
    #[error("ranh giới fork {boundary} không hợp lệ: {reason}")]
    InvalidBoundary { boundary: Seq, reason: &'static str },

    /// Cắt giữa một lượt đang mở sẽ sinh ra phiên con có `turn/start` không bao giờ được
    /// đóng — một sổ mà chính bộ phát lại cũng không đọc được.
    #[error("ranh giới {boundary} nằm giữa lượt {turn} đang mở (có turn/start, chưa có turn/end)")]
    OpenTurn { boundary: Seq, turn: u64 },

    #[error("sự kiện surface `{0}` bắt buộc phải mang surface_op")]
    SurfaceOpRequired(&'static str),

    #[error("sự kiện log-only `{0}` không được mang surface_op hay source_event_seqs")]
    SurfaceOpForbidden(&'static str),

    #[error("dải replace {start}..{end} nằm ngoài {len} node surface hiện có")]
    SurfaceRangeOutOfBounds {
        start: usize,
        end: usize,
        len: usize,
    },

    /// Một `replace` không kê đủ node bị che sẽ khiến bản ghi mất dấu vết: về sau không
    /// còn cách nào biết bản tóm tắt đã nuốt những gì.
    #[error("replace phải kê mọi node bị che trong source_event_seqs; thiếu {missing:?}")]
    UncitedShadow { missing: Vec<Seq> },

    /// `seq` liền mạch là bất biến trung tâm. Một lỗ hổng nghĩa là sổ đã mất event, và
    /// mọi thứ chiếu ra từ nó — kể cả lịch sử gửi cho mô hình — đều không còn tin được.
    #[error("seq không liền mạch: chờ {expected}, gặp {found}")]
    SeqGap { expected: Seq, found: Seq },

    /// Reader gặp loại sự kiện nó không hiểu và loại đó không tự nhận là bỏ qua được.
    /// Im lặng lướt qua sẽ dựng lại một lịch sử thiếu, mà mô hình thì không biết.
    #[error("phiên dùng loại sự kiện `{0}` mà bản này không hiểu; hãy nâng cấp ứng dụng")]
    FormatUnsupported(String),

    #[error("tệp này không phải kho phiên của pai (application_id={found:#x})")]
    NotOurStore { found: i32 },

    /// Không migrate ngầm: một schema lệch phiên bản là quyết định của con người.
    #[error("kho phiên ở schema v{found}, bản này nói v{expected}")]
    SchemaVersion { found: i32, expected: i32 },

    /// Hai tiến trình cùng ghi một phiên. Ghi đè sẽ tạo ra lỗ hổng seq.
    #[error("phiên `{0}` đã bị ghi bởi nơi khác kể từ lần đọc gần nhất")]
    ConcurrentWrite(String),

    #[error("lỗi sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("dữ liệu sự kiện không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),

    /// Khoá của kho đã nhiễm độc vì một luồng khác hoảng loạn khi đang giữ nó. Trả về
    /// lỗi thay vì hoảng loạn tiếp: một phiên hỏng không được kéo cả ứng dụng theo.
    #[error("kho phiên không dùng được nữa: {0}")]
    Unavailable(String),
}
