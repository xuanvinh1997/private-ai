//! Tầng dây: byte đến từ socket, chunk đi ra.
//!
//! Một sự thật chi phối cả thư mục này: **một lần đọc socket không phải một đơn vị của
//! giao thức.** TCP cắt ở đâu là chuyện của TCP. Một event SSE có thể đến làm ba lần
//! đọc; một dòng NDJSON có thể bị cắt ngang; và điểm cắt có thể rơi vào giữa một ký tự
//! UTF-8 nhiều byte, giữa một escape `\"`, hay giữa `\r` và `\n`.
//!
//! Vì thế mọi bộ giải mã ở đây đệm **byte**, không đệm chuỗi, và chỉ giải mã UTF-8 khi
//! đã có trọn một dòng. Đảo thứ tự đó lại là cách chắc chắn nhất để làm hỏng tiếng Việt.

pub mod ndjson;
pub mod pump;
pub mod sse;

pub use ndjson::LineDecoder;
pub use pump::{FrameDecoder, pump};
pub use sse::{SseDecoder, SseEvent};
