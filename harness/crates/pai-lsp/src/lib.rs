//! Cầu nối Language Server Protocol: những câu hỏi về mã mà cú pháp không trả lời được.
//!
//! # Ranh giới với `pai-index`
//!
//! Ranh giới phải rõ, nếu không ta có hai tool cho một câu hỏi và mô hình phải đoán.
//!
//! `pai-index` là **tree-sitter**: nó biết một cái tên được *khai báo* ở đâu chỉ bằng hình
//! dạng của tệp — offline, không cần cài gì, vài mili giây, đúng cho mọi repo.
//! `symbol_search` và `outline` là của nó, và **crate này không được làm lại chúng**.
//!
//! Cái nó không làm được là mọi thứ cần một trình biên dịch: đi tới định nghĩa qua `use` và
//! qua nhiều tệp khi hai hàm trùng tên, tìm mọi nơi *tham chiếu* thay vì mọi nơi trùng chữ,
//! kiểu suy ra được, và lỗi biên dịch thật. Đó là bốn thao tác của tool `lsp` — không hơn.
//!
//! # Ràng buộc: server có thể không tồn tại
//!
//! Language server là tiến trình bên ngoài mà người dùng có thể chưa cài. Ba hệ quả, cả ba
//! đều là mã chứ không phải lời hứa:
//!
//! 1. **Không dò được server nào thì tool không được đăng ký** ([`plugin`]). Một tool lần
//!    nào gọi cũng lỗi dạy mô hình bỏ qua cả danh sách.
//! 2. **Dò một lần, lúc cắm plugin** ([`launch::locate`]), không dò lại mỗi lần gọi.
//! 3. **Chờ có hạn, rồi nói thật.** `rust-analyzer` mất hàng chục giây để nạp workspace.
//!    Lần gọi đầu chờ tối đa [`config::STARTUP_TIMEOUT`] rồi báo server chưa sẵn sàng —
//!    không treo, và **không** trả rỗng, vì rỗng đọc y hệt "hàm đó không tồn tại". Trong
//!    lúc còn lập chỉ mục, mọi câu trả lời kèm lời nhắc rằng nó có thể còn thiếu; cơ sở là
//!    `$/progress` của spec, xem [`client`].
//!
//! # Hình dạng
//!
//! ```text
//! seam      seam `Lsp` + từ vựng câu hỏi/câu trả lời. Toạ độ 1-based ở biên này
//! config    bảng ngôn ngữ: thêm một server = thêm một hàng. Giao thức thì chung
//! launch    mở ống tới một server + dò lệnh trên `PATH`
//! proto     khung tin `Content-Length` của JSON-RPC
//! client    một kết nối: bắt tay, hỏi–đáp, chết cho tử tế
//! pool      provider stdio: vòng đời, đường dẫn ↔ URI, toạ độ ↔ LSP
//! tool      tool `lsp` với bốn thao tác
//! plugin    cắm vào cây, hoặc cố ý không cắm
//! ```
//!
//! Phần khó — JSON-RPC với tiến trình con qua stdio, giám sát vòng đời, và không để một
//! server bên ngoài chết làm hỏng ứng dụng — đã được `pai-mcp` giải xong, và cách làm ở đây
//! chép của nó: một trait mở-kết-nối để bài kiểm chứng thay được bằng ống trong bộ nhớ, một
//! chủ duy nhất cho mỗi kết nối, và mọi lỗi phía ngoài dừng trước khi chạm tool người dùng.

pub mod client;
pub mod config;
pub mod error;
pub mod launch;
pub mod plugin;
pub mod pool;
pub mod proto;
pub mod seam;
pub mod tool;
pub mod uri;

pub use client::Client;
pub use config::{LanguageConfig, Limits, defaults, language_id};
pub use error::LspError;
pub use launch::{Channel, ChildLaunch, Launch, locate};
pub use plugin::LspPlugin;
pub use pool::{Entry, StdioServers};
pub use seam::{Answer, Hit, LanguageServers, Lsp, Note, Operation, Query};
pub use tool::{LspArgs, LspTool};
pub use uri::{UriError, from_uri, to_uri};
