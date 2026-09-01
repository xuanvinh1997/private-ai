//! Chỉ mục mã nguồn: tree-sitter ra ký hiệu, SQLite + FTS5 để tra.
//!
//! # Không có embedding, không có mô hình
//!
//! Chỉ mục này là **cú pháp và ký hiệu**, không phải ngữ nghĩa. Nó không gọi LLM, không
//! sinh vector, và không có chỗ nào để cắm một cái vào. Ba lý do, theo thứ tự quan trọng:
//!
//! 1. **Một chỉ mục cần mô hình thì hỏng đúng lúc mô hình không chạy.** Ollama chưa bật,
//!    máy đang hết VRAM, người dùng vừa đổi sang một model không có embedding — đó là
//!    những lúc người ta cần tìm một hàm nhất, và là những lúc một chỉ mục ngữ nghĩa im
//!    lặng trả về rỗng.
//! 2. **Với mã nguồn, tên ký hiệu cộng cấu trúc đã mang gần hết thông tin.** Người ta đi
//!    tìm `resolve_read`, không đi tìm "chỗ kiểm tra đường dẫn"; và khi người ta thật sự
//!    đi tìm theo ý nghĩa thì `grep` với một mô tả cụ thể đã đủ xa. Đồ thị AST là thứ có
//!    thật trong tệp, còn đồ thị thực thể do mô hình sinh ra là một phỏng đoán được lưu
//!    lại như thể nó là sự thật — đó là lý do LightRAG dừng lại ở bản Python.
//! 3. **Nó cho phép chỉ mục tăng dần rẻ tới mức chạy được trước mỗi lần hỏi.** Không có
//!    bước sinh vector thì một tệp không đổi tốn đúng một lần `stat`.
//!
//! # Hình dạng
//!
//! ```text
//! lang      bảng ngôn ngữ + truy vấn. Thêm một ngôn ngữ = thêm một hàng
//! extract   cây cú pháp -> ký hiệu, cha–con suy từ bao hàm phạm vi byte
//! store     SQLite: `files` (mtime + kích thước), `symbols`, và một bảng FTS5
//! index     seam `Index` + bản cài đặt quét trên đĩa. Tăng dần là bất biến của nó
//! tools     `symbol_search` và `outline`
//! ```
//!
//! Hai chỗ bản Python thua, và cả hai đều rẻ ở đây: nó **không có FTS5** nên mỗi câu hỏi
//! là một lần quét toàn bộ, và nó **không có chỉ mục tăng dần** nên mỗi lần quét là một
//! lần đọc lại mọi tệp.

pub mod error;
pub mod extract;
pub mod index;
pub mod lang;
pub mod plugin;
pub mod store;
pub mod symbol;
pub mod tools;

pub use error::IndexError;
pub use extract::Extractor;
pub use index::{CodeIndex, Index, SymbolIndex, SyncReport};
pub use lang::{LANGUAGES, Lang};
pub use plugin::IndexPlugin;
pub use store::Store;
pub use symbol::{Symbol, SymbolKind};
