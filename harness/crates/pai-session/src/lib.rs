//! Append-only session log: the single source of the context the model sees.
//! Anything the model sees must be reconstructible from the log, so history is projected from it
//! rather than kept alongside, compaction only shadows nodes, and `seq` must have no gaps.

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
    Boundary, NewSession, NoTitle, Origin, SessionHeader, SessionId, SessionScope, SessionStore,
    SessionTitle, SessionTitler, Sessions, new_session_id,
};
pub use surface::{Surface, SurfaceOp};
