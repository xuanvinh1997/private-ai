//! Thư viện tài liệu: nạp nhiều loại tệp, rồi hỏi đáp trên chúng.
//!
//! Đây là tầng của **loại dự án thứ hai**. Dự án mã nguồn có `pai-index`: tree-sitter ra
//! ký hiệu, không có mô hình nào trong đường chạy. Dự án tài liệu thì ngược lại — không
//! có cấu trúc nào để dựa vào, chỉ có chữ — nên ở đây có nhúng vector, và cùng với nó là
//! một ràng buộc mà `pai-index` không có: **mô hình có thể vắng mặt.**
//!
//! # Bất biến trung tâm
//!
//! > Nạp tài liệu **không bao giờ** phụ thuộc vào việc bộ nhúng có chạy hay không.
//!
//! Không có bộ nhúng, hoặc Ollama chưa bật, thì tài liệu vẫn được rút chữ, cắt đoạn và
//! đưa vào FTS5; tìm bằng từ khoá chạy ngay lập tức, và [`Stats::reason`] nói ra bằng
//! tiếng Việt vì sao phần tìm theo ý nghĩa chưa có. Đây là yêu cầu sản phẩm chứ không
//! phải một lựa chọn kỹ thuật: người dùng vừa thả hai mươi tệp vào cửa sổ, và "không có
//! gì xảy ra" là câu trả lời tệ nhất có thể đưa cho họ.
//!
//! # Hình dạng
//!
//! ```text
//! extract   tệp -> chữ. PDF, DOCX, HTML, CSV, markdown, mã nguồn. Một tệp hỏng chỉ
//!           làm hỏng chính nó — kể cả khi nó làm `pdf-extract` hoảng loạn
//! chunk     chữ -> đoạn, theo ranh giới ngữ nghĩa. Offset byte luôn ở ranh giới ký tự
//! embed     seam `Embeddings` + hai bản cài đặt: Ollama và OpenAI-compatible
//! store     SQLite: `documents`, `chunks`, FTS5 external content, và bảng `vectors`
//! search    hợp nhất BM25 với cosine bằng Reciprocal Rank Fusion
//! library   seam `Docs`: quét thư mục dự án, nạp có tiến trình, liệt kê, bỏ khỏi chỉ
//!           mục, tìm. Thư mục dự án **là** thư viện — xem tài liệu của module
//! tools     `docs.search`, `docs.read`, `docs.list` — cả ba trả nội dung không đáng tin
//! ```
//!
//! # Ranh giới tin cậy
//!
//! Tài liệu do người dùng nạp lên là **dữ liệu từ ngoài vào**. Cả ba tool khai
//! `returns_untrusted_content`, nên sổ đăng ký chèn lời cảnh báo vào mô tả của chúng — và
//! không có đường nào từ tầng này tạo, đặt tên hay sửa một skill. Xem `docs/CONTRACT.md`,
//! luật 7.

pub mod chunk;
pub mod embed;
pub mod error;
pub mod extract;
pub mod library;
pub mod plugin;
pub mod search;
pub mod store;
pub mod tools;

pub use chunk::{Chunk, ChunkOpts, chunk};
pub use embed::{Embedder, Embeddings, MAX_BATCH, OllamaEmbedder, OpenAiEmbedder};
pub use error::RagError;
pub use extract::{Extracted, Format, MAX_FILE_BYTES, extract, format_for};
pub use library::{
    DocLibrary, Docs, Document, Hit, IngestEvent, IngestStage, Library, MAX_FILES, Scanning, Stats,
};
pub use plugin::RagPlugin;
pub use search::{MatchedBy, RRF_K, cosine, fuse, rank_by_cosine};
pub use store::Store;
