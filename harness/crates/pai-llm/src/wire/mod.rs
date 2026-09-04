//! Wire layer: bytes in from the socket, chunks out.
//! One socket read is not a protocol unit, and a split can land mid UTF-8 character, so
//! every decoder here buffers *bytes* and only decodes UTF-8 once a full line is in hand.

pub mod ndjson;
pub mod pump;
pub mod sse;

pub use ndjson::LineDecoder;
pub use pump::{FrameDecoder, pump};
pub use sse::{SseDecoder, SseEvent};
