//! Session log errors. Each variant is a violated invariant, kept separate so callers can tell
//! "this log is broken, stop writing" from "your request was wrong, fix it and retry".

use crate::event::Seq;

pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("không tìm thấy phiên `{0}`")]
    NotFound(String),

    #[error("phiên `{0}` đã tồn tại")]
    AlreadyExists(String),

    /// Bad fork boundary; never rounded, since rounding would create a child session nobody asked for.
    #[error("ranh giới fork {boundary} không hợp lệ: {reason}")]
    InvalidBoundary { boundary: Seq, reason: &'static str },

    /// Cutting inside an open turn would leave a child with a `turn/start` that never closes.
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

    /// A `replace` that does not cite every shadowed node loses the trace of what the summary swallowed.
    #[error("replace phải kê mọi node bị che trong source_event_seqs; thiếu {missing:?}")]
    UncitedShadow { missing: Vec<Seq> },

    /// Gapless `seq` is the central invariant: a gap means lost events, so every projection is untrustworthy.
    #[error("seq không liền mạch: chờ {expected}, gặp {found}")]
    SeqGap { expected: Seq, found: Seq },

    /// An unknown event type that does not declare itself skippable; skipping it would rebuild a silently incomplete history.
    #[error("phiên dùng loại sự kiện `{0}` mà bản này không hiểu; hãy nâng cấp ứng dụng")]
    FormatUnsupported(String),

    #[error("tệp này không phải kho phiên của pai (application_id={found:#x})")]
    NotOurStore { found: i32 },

    /// No implicit migration: a schema version mismatch is a human decision.
    #[error("kho phiên ở schema v{found}, bản này nói v{expected}")]
    SchemaVersion { found: i32, expected: i32 },

    /// Two processes writing the same session; overwriting would create a seq gap.
    #[error("phiên `{0}` đã bị ghi bởi nơi khác kể từ lần đọc gần nhất")]
    ConcurrentWrite(String),

    #[error("lỗi sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("dữ liệu sự kiện không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),

    /// The store lock was poisoned by a panicking thread; we return an error so one broken session cannot take down the app.
    #[error("kho phiên không dùng được nữa: {0}")]
    Unavailable(String),
}
