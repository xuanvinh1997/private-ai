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
//! lang      bảng ngôn ngữ + hai truy vấn. Thêm một ngôn ngữ = thêm một hàng
//! extract   cây cú pháp -> ký hiệu + tham chiếu, cả hai suy từ bao hàm phạm vi byte
//! graph     từ vựng của đồ thị, và chỗ nói ra rằng cạnh là phỏng đoán theo tên
//! store     SQLite: `files`, `symbols`, FTS5, `refs` (chưa phân giải), `edges` (đã)
//! index     seam `Index` + bản cài đặt quét trên đĩa. Tăng dần là bất biến của nó
//! tools     `symbol_search`, `outline`, `code.graph`, `code.trace`, `code.overview`
//! ```
//!
//! # Đồ thị, và mức độ nó dám hứa
//!
//! Đỉnh là ký hiệu, cạnh là quan hệ. Đúng **một** loại cạnh là sự thật cú pháp —
//! `contains`; năm loại còn lại là phỏng đoán theo tên, vì không có phân tích kiểu ở đây.
//! Điều đó không được giấu đi ở bất kỳ tầng nào: xem [`graph::NAME_BASED_NOTICE`], thứ đi
//! kèm mọi kết quả tool. Cùng lý do khiến `pai-sandbox` báo `Enforcement::Partial` thay
//! vì làm tròn lên thành "có giam".
//!
//! Hai chỗ bản Python thua, và cả hai đều rẻ ở đây: nó **không có FTS5** nên mỗi câu hỏi
//! là một lần quét toàn bộ, và nó **không có chỉ mục tăng dần** nên mỗi lần quét là một
//! lần đọc lại mọi tệp.

pub mod error;
pub mod extract;
pub mod graph;
pub mod index;
pub mod lang;
pub mod plugin;
pub mod store;
pub mod symbol;
pub mod tools;

pub use error::IndexError;
pub use extract::{Extraction, Extractor};
pub use graph::{
    CentralSymbol, DirectorySummary, EdgeKind, GraphEdge, GraphNode, NAME_BASED_NOTICE,
    Neighborhood, Overview, Reference, Stats,
};
pub use index::{CodeIndex, Index, MAX_DEPTH, MAX_NODES, MAX_PATHS, SymbolIndex, SyncReport};
pub use lang::{LANGUAGES, Lang};
pub use plugin::IndexPlugin;
pub use store::Store;
pub use symbol::{Symbol, SymbolKind};
