//! Tool đi kèm crate này.
//!
//! Chỉ những cái không phụ thuộc vào hệ tệp, tiến trình hay mạng. Đọc/ghi tệp ở `pai-fs`,
//! chạy lệnh ở `pai-shell` — không phải để cho gọn, mà vì mỗi crate đó kéo theo một seam
//! phải giam giữ được, và trộn chúng vào đây sẽ khiến crate nền tảng bảo mật phụ thuộc
//! vào chính những thứ nó phải canh.

pub mod spill_read;
pub mod todo;

pub use spill_read::{SpillRead, SpillReadArgs};
pub use todo::{TodoItem, TodoStatus, TodoWrite, TodoWriteArgs};
