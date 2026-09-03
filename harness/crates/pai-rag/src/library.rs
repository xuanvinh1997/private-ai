//! Seam thư viện tài liệu: những kiểu mà tầng trên nhìn thấy, và hợp đồng của chúng.
//!
//! # Chỗ này từng là gì
//!
//! Trước đây module này **là** thư viện: quét thư mục, rút chữ, cắt đoạn, nhúng, tìm —
//! khoảng một nghìn năm trăm dòng. Giờ nó chỉ còn là hợp đồng, còn phần thi hành nằm ở
//! `services/rag/`, một tiến trình Python nói MCP qua stdio.
//!
//! Đổi như vậy vì ba việc mà Rust ở đây không làm nổi ở mức đáng dùng: đọc DOCX/XLSX/PPTX
//! (markitdown), OCR bản quét bằng mô hình vision, và xếp hạng lại bằng cross-encoder.
//! Xem `services/rag/README.md`.
//!
//! # Vì sao mọi phương thức đều `async`
//!
//! Bản cũ để `documents`, `chunks`, `stats` và `remove` đồng bộ, vì chúng chỉ là vài câu
//! truy vấn SQLite trong cùng tiến trình. Giờ mỗi cái là một vòng gọi tới một tiến trình
//! khác. Giữ chúng đồng bộ thì bản cài đặt buộc phải `block_on` bên trong — chặn một
//! thread của runtime, và trong runtime của Tauri thì đó là đường thẳng tới deadlock.
//!
//! Mọi chỗ gọi đã nằm trong lệnh Tauri `async`, nên cái giá thật sự chỉ là thêm `.await`.

use std::path::PathBuf;

use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_core::ServiceKey;
use serde::{Deserialize, Serialize};

use crate::error::RagError;
use crate::format::Format;
use crate::search::MatchedBy;

/// Bao nhiêu tệp một lần quét chịu nạp.
///
/// Giữ ở đây dù việc thi hành nằm bên Python: giao diện nói ra con số này khi một lần
/// quét chạm trần, và hai bản sao của cùng một con số ở hai ngôn ngữ thì sớm muộn lệch
/// nhau. Bên Python đọc nó từ cấu hình do phía này ghi ra.
pub const MAX_FILES: usize = 5_000;

/// Một tài liệu như tầng trên thấy nó. Chuyển sang `DocumentView` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Document {
    pub id: String,
    /// Tệp thật trong thư mục dự án. Đây là chỗ người dùng mở được bằng Explorer.
    pub path: PathBuf,
    /// Chỗ tệp đến từ đó. Bằng `path` với tệp vốn đã nằm trong thư mục dự án.
    pub origin: String,
    pub title: String,
    pub format: Format,
    pub bytes: u64,
    pub chunks: u32,
    pub embedded: bool,
    pub added_at: i64,
    /// `None` cộng `embedded == false` nghĩa là **đang xếp hàng**, không phải hỏng.
    pub error: Option<String>,
    /// Số trang, khi định dạng có khái niệm ấy.
    pub pages: u32,
    /// Trang nào phải đọc bằng OCR. Giao diện nói "12/40 trang đọc bằng OCR" từ đây, và
    /// đó là câu giải thích vì sao một tệp nạp lâu hơn hẳn những tệp khác.
    pub ocr_pages: Vec<u32>,
}

/// Một lượt quét đang chạy, đủ để giao diện nói "đang quét 12/240 tệp".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scanning {
    pub done: u32,
    pub total: u32,
}

/// Sức khoẻ của thư viện. Chuyển sang `LibraryStats` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Stats {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
    pub embedder: Option<String>,
    pub semantic_ready: bool,
    /// Câu tiếng Việt giải thích khi `semantic_ready` là `false`. Đây là chỗ duy nhất
    /// người dùng biết được vì sao kết quả của họ chỉ có từ khoá — hoặc vì sao thư viện
    /// trống trong khi thư mục thì không.
    pub reason: Option<String>,
    /// Thư mục tài liệu của người dùng. Giao diện phải chỉ ra được nó: câu hỏi "vì sao
    /// không thấy tệp nào" bắt đầu bằng việc người dùng kiểm lại họ đã chỉ vào đâu.
    pub root: PathBuf,
    pub files_seen: u32,
    pub files_skipped: u32,
    /// Số tệp đã thử đọc và không đọc được.
    pub unreadable: u32,
    pub excluded: u32,
    pub scanned_at: Option<i64>,
    pub scanning: Option<Scanning>,
}

/// Một đoạn khớp. Chuyển sang `DocumentHit` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Hit {
    pub document_id: String,
    pub title: String,
    pub path: PathBuf,
    pub ordinal: u32,
    pub heading: Option<String>,
    pub text: String,
    pub score: f32,
    pub matched_by: MatchedBy,
    /// Trang chứa đoạn này, `0` khi định dạng không có trang. Đi vào trích dẫn: một câu
    /// trả lời chỉ được ra trang mấy thì người dùng kiểm chứng được trong vài giây.
    pub page: u32,
}

/// Giai đoạn của một tệp trong lúc nạp. Chuyển sang `IngestProgress.stage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStage {
    Reading,
    Stored,
    Failed,
    /// Tệp bị bỏ qua **có lý do**: quá lớn, hoặc nằm ngoài trần số tệp. Tách khỏi
    /// `Failed` vì đây không phải một tệp hỏng — nó lành lặn, thư viện mới là bên từ chối.
    Skipped,
    Removed,
    /// Đợt nhúng bù ở cuối mỗi lượt. Không thuộc về tệp nào; tách khỏi `Failed` để giao
    /// diện không đếm nó vào danh sách *tệp* hỏng: "1 tệp không nạp được" là một câu sai
    /// khi mọi tệp đều đã vào và chỉ có máy chủ nhúng là chưa trả lời.
    Embedding,
    /// Cả mẻ đã xong. Luôn là sự kiện cuối cùng của dòng.
    Finished,
}

impl IngestStage {
    pub fn as_str(self) -> &'static str {
        match self {
            IngestStage::Reading => "reading",
            IngestStage::Stored => "stored",
            IngestStage::Failed => "failed",
            IngestStage::Skipped => "skipped",
            IngestStage::Removed => "removed",
            IngestStage::Embedding => "embedding",
            IngestStage::Finished => "finished",
        }
    }
}

/// Một mốc tiến trình. Chuyển sang `IngestProgress` phía `app/`.
#[derive(Clone, Debug)]
pub struct IngestEvent {
    pub path: String,
    pub stage: IngestStage,
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    pub error: Option<String>,
    /// Tài liệu vừa xong. Có để giao diện thêm được một hàng mà không phải hỏi lại cả
    /// danh sách sau mỗi tệp — với hai trăm tệp thì đó là hai trăm lần vẽ lại.
    pub document: Option<Document>,
}

/// Cái mà tool và tầng trên nhìn thấy.
///
/// # Vì sao `sync`, `ingest` và `remove` nằm ở đây
///
/// Chúng là **lệnh của giao diện**, không phải tool của mô hình — không có tool nào nạp
/// hay xoá tài liệu, và đó là có chủ ý (xem [`crate::tools`]). Nhưng chúng vẫn phải nằm
/// trên seam, vì thiếu chúng thì tầng trên chỉ cầm được một `Arc<dyn DocLibrary>` và
/// buộc phải mở một đường thứ hai tới service để quét — hai đường tới cùng một thư viện
/// là hai chỗ để cấu hình lệch nhau.
#[async_trait]
pub trait DocLibrary: Send + Sync + 'static {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError>;
    async fn documents(&self) -> Result<Vec<Document>, RagError>;
    /// Đọc liền mạch một tài liệu theo đoạn.
    async fn chunks(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Hit>, RagError>;
    async fn stats(&self) -> Result<Stats, RagError>;
    /// Bắt kịp thư mục dự án. Đường vào chính của một dự án tài liệu.
    fn sync(&self) -> BoxStream<'_, IngestEvent>;
    /// Nạp một danh sách tệp cụ thể. Dòng mượn `&self`, nên nó không sống lâu hơn thư
    /// viện — một dòng còn chạy sau khi kết nối đã đóng là một dòng ghi vào chỗ trống.
    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent>;
    /// Quên mọi dấu vân tay rồi đọc lại cả thư mục.
    fn reprocess(&self) -> BoxStream<'_, IngestEvent>;
    /// Bỏ một tài liệu khỏi thư viện. **Không** xoá tệp trên đĩa.
    async fn remove(&self, id: &str) -> Result<(), RagError>;
}

/// Seam thư viện tài liệu.
pub enum Docs {}
impl ServiceKey for Docs {
    type Api = dyn DocLibrary;
    const NAME: &'static str = "rag.docs";
}
