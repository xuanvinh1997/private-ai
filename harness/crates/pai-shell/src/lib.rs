//! Thi hành lệnh: một seam, bốn tool, một canh gác.
//!
//! Hai ý đáng nhớ:
//!
//! **Cái ta chạy là một cây tiến trình.** `sh -c "npm test"` sinh ra `npm`, sinh ra
//! `node`. Giết cái shell để lại cả hai đứa kia giữ cổng và giữ khoá tệp. Nên mọi lệnh
//! chạy trong nhóm tiến trình riêng và mọi tín hiệu gửi cho cả nhóm. Xem [`provider`].
//!
//! **Không có danh sách đen.** Lọc lệnh nguy hiểm bằng cách so chuỗi luôn thủng, và cái
//! nó tạo ra không phải an toàn mà là cảm giác an toàn — thứ khiến người ta bấm "cho
//! phép" mà không đọc. Phòng thủ thật là duyệt (ở đây) và giam tiến trình (`pai-sandbox`).

pub mod jobs;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use jobs::{Job, JobState, Jobs};
pub use plugin::ShellPlugin;
pub use provider::{Execution, LocalShell, Request, Shell, ShellError, ShellExecutor};
