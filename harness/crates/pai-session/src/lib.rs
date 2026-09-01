//! Sổ tay phiên chỉ-ghi-thêm — nguồn duy nhất của ngữ cảnh mà mô hình thấy.
//!
//! Bất biến trung tâm:
//!
//! > **Cái gì mô hình thấy được thì phải nằm trong sổ.** Mọi thứ đi vào một request đều
//! > phải dựng lại được từ sổ. Vì thế thêm một loại đầu vào mới là thêm một loại sự kiện
//! > mới, không phải thêm một trường ở đâu đó ngoài sổ.
//!
//! Hệ quả kiến trúc:
//!
//! - Không có `history: Vec<Message>` sống song song. Lịch sử được **chiếu** ra từ sổ
//!   bằng [`SessionLog::derive_messages`], và chỉ ba loại sự kiện sinh ra message.
//! - Nén ngữ cảnh **không xoá gì cả**: nó ghi thêm một sự kiện mang
//!   [`SurfaceOp::Replace`] che một dải node. Bản ghi vẫn phát lại được nguyên vẹn.
//! - Ghi vào sổ là chỉ-ghi-thêm với `seq` **liền mạch**. Một lỗ hổng trong `seq` nghĩa là
//!   đã mất sự kiện, và mọi thứ chiếu ra từ đó đều không còn tin được.

pub mod error;
pub mod event;
pub mod log;
pub mod message;
pub mod plugin;
pub mod session;
pub mod sqlite;
pub mod store;
pub mod surface;

pub use error::{Result, SessionError};
pub use event::{
    AssistantChunk, AssistantMessage, RequestHeader, RequestReason, SESSION_FORMAT_VERSION,
    SURFACE_TYPES, Seq, SessionEvent, SessionEventEnvelope, StepEnd, StepStart, ToolCall,
    ToolErrorInfo, ToolResult, TurnEnd, TurnEndReason, TurnStart, UnknownEvent, Usage,
};
pub use log::SessionLog;
pub use message::{ContentBlock, Message, Role};
pub use plugin::SessionPlugin;
pub use session::{Session, SessionService};
pub use sqlite::SqliteSessionStore;
pub use store::{
    Boundary, NewSession, NoTitle, Origin, SessionHeader, SessionId, SessionStore, SessionTitle,
    SessionTitler, Sessions, new_session_id,
};
pub use surface::{Surface, SurfaceOp};
