//! Thư viện tài liệu: nạp nhiều loại tệp, rồi hỏi đáp trên chúng.
//!
//! Đây là tầng của **loại dự án thứ hai**. Dự án mã nguồn có `pai-index`: tree-sitter ra
//! ký hiệu, không mô hình nào trong đường chạy. Dự án tài liệu thì ngược lại — không có
//! cấu trúc để dựa vào, chỉ có chữ — nên ở đây có nhúng vector, xếp hạng lại, và OCR.
//!
//! # Crate này giờ là một client
//!
//! Phần thi hành nằm ở `services/rag/`: một tiến trình Python nói MCP qua stdio. Ba việc
//! đẩy nó sang đó, và cả ba đều là chỗ hệ sinh thái Python đi trước rất xa:
//!
//! - **Đọc định dạng.** `markitdown` mở DOCX, XLSX, PPTX, HTML, CSV. Viết lại chúng bằng
//!   Rust là viết lại một thư viện lớn để đọc một tệp.
//!   pai-rag cũ chỉ đọc được PDF, DOCX và văn bản.
//! - **OCR bản quét.** Một PDF quét trước đây vào thư viện với **0 đoạn** rồi nằm vĩnh
//!   viễn trong bảng lỗi, vì `mtime` không đổi nên không lần quét nào chạm lại. Giờ nó
//!   được dựng thành ảnh trang và đọc bằng mô hình vision.
//! - **Xếp hạng lại.** Cross-encoder chạy trên ONNX Runtime. BM25 và cosine là hai tín
//!   hiệu rẻ dùng để thu hẹp; cross-encoder mới là thứ đọc cả câu hỏi lẫn đoạn cùng lúc.
//!
//! # Hình dạng
//!
//! ```text
//! sidecar   dựng tiến trình con và giữ kết nối MCP; nối lười, nối lại được
//! client    `DocLibrary` gọi tool `docs.*` và đọc JSON trả về
//! library   seam: `Document`, `Hit`, `Stats`, `IngestEvent`, và trait `DocLibrary`
//! format    định dạng tài liệu — phải khớp với nhãn bên Python và union bên UI
//! search    `MatchedBy`: vì sao một đoạn có mặt trong kết quả
//! tools     `docs.search`, `docs.read`, `docs.list` — cả ba trả nội dung không đáng tin
//! ```
//!
//! # Ranh giới tin cậy
//!
//! Không đổi so với bản trước, và đó là điểm chính: tài liệu người dùng nạp lên là **dữ
//! liệu từ ngoài vào**. Cả ba tool khai `returns_untrusted_content`, nên sổ đăng ký chèn
//! cảnh báo vào mô tả của chúng — và không có đường nào từ tầng này tạo, đặt tên hay sửa
//! một skill.
//!
//! Service phơi thêm bốn tool quản lý — `docs.sync`, `docs.ingest`, `docs.reprocess`,
//! `docs.remove` — nhưng [`plugin`] **không** đăng ký chúng vào sổ tool. Chúng chỉ tới
//! được qua lệnh Tauri, tức là qua một hành động của con người. Nếu mô hình nạp hay xoá
//! được tài liệu thì một tài liệu không đáng tin có thể bảo nó làm việc đó.

pub mod client;
pub mod error;
pub mod format;
pub mod library;
pub mod plugin;
pub mod search;
pub mod sidecar;
pub mod tools;

pub use client::RagClient;
pub use error::RagError;
pub use format::Format;
pub use library::{
    DocLibrary, Docs, Document, Hit, IngestEvent, IngestStage, MAX_FILES, Scanning, Stats,
};
pub use plugin::RagPlugin;
pub use search::MatchedBy;
pub use sidecar::{Sidecar, SidecarConfig};
