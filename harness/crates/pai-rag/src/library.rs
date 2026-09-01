//! Thư viện tài liệu: quét thư mục dự án, nạp, liệt kê, bỏ khỏi thư viện, tìm.
//!
//! # Ba quyết định sản phẩm, không phải ba tuỳ chọn kỹ thuật
//!
//! **1. Thư mục dự án *là* thư viện.** Người dùng mở `~/Tài liệu/NCS` thành một dự án tài
//! liệu; tệp trong đó là nguồn sự thật, còn kho chỉ là chỉ mục soi vào nó. Bản trước bắt
//! họ thêm từng tệp một qua nút "Chọn tệp…" rồi chép bản sao vào một thư mục ẩn, nên màn
//! hình thư viện hiện **0 tài liệu** ngay sau khi họ vừa chỉ đúng chỗ tài liệu của mình
//! nằm. Đó là câu trả lời tệ nhất mà phần mềm này có thể đưa ra, và nó xuất hiện ở đúng
//! bước đầu tiên.
//!
//! Dự án mã nguồn đã làm đúng từ đầu — `pai-index` quét thư mục và bắt kịp đĩa theo
//! `mtime` + kích thước — nên [`Library::sync`] chép lại đúng hình dạng ấy, kể cả việc
//! dùng crate `ignore` để tôn trọng `.gitignore` với `require_git(false)`.
//!
//! **2. Bỏ một tài liệu khỏi thư viện *không* xoá tệp của người dùng.** Xem
//! [`Library::remove`]. Trước đây `remove` xoá một bản sao trong kho ẩn nên nó vô hại;
//! giờ đường dẫn trỏ vào tệp thật, và cùng một dòng lệnh sẽ là một hành động không lấy
//! lại được.
//!
//! **3. Nhúng vector là bước được phép hỏng.** Khi không có bộ nhúng, hoặc khi Ollama chưa
//! bật, tài liệu **vẫn** được rút chữ, cắt đoạn và đưa vào FTS5 — tìm bằng từ khoá chạy
//! ngay. Lý do nằm ở phía người dùng chứ không ở phía kiến trúc: họ vừa chỉ vào một thư
//! mục hai trăm tệp, và "không có gì xảy ra" là câu trả lời tệ nhất có thể đưa cho họ. Cái
//! chưa xong được nói ra ở [`Stats::reason`], và [`Library::embed_pending`] dọn nốt khi bộ
//! nhúng quay lại.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use ignore::WalkBuilder;
use pai_core::ServiceKey;
use serde::{Deserialize, Serialize};

use crate::chunk::{ChunkOpts, chunk};
use crate::embed::{Embedder, MAX_BATCH};
use crate::error::RagError;
use crate::extract::{Format, extract, format_for};
use crate::search::{MatchedBy, fuse, rank_by_cosine};
use crate::store::{self, ChunkRow, DocumentRow, FileState, Opened, Store};

/// Tên tệp cơ sở dữ liệu bên trong thư mục kho.
const DB_NAME: &str = "library.sqlite";

/// Bao nhiêu tệp một lần quét chịu nạp.
///
/// Người dùng chỉ vào thư mục Downloads mười nghìn tệp là chuyện có thật, và mười nghìn
/// lần rút chữ cộng mười nghìn lần gọi bộ nhúng thì không phải một lần chờ lâu — nó là một
/// ứng dụng đứng hình cả buổi. Trần này cắt ở chỗ vẫn còn dùng được, và khi chạm trần thì
/// [`Library::sync`] **nói ra** qua một [`IngestEvent`] thay vì lặng lẽ dừng: một thư viện
/// thiếu tệp mà không giải thích là đúng cái lỗi mà cả module này sinh ra để sửa.
pub const MAX_FILES: usize = 5_000;

/// Bao nhiêu tệp bị bỏ qua được kể tên trước khi gộp phần còn lại thành một câu.
///
/// Một thư mục ảnh nghìn tệp quá lớn sẽ sinh ra nghìn sự kiện, và nghìn dòng cảnh báo nói
/// ít hơn hai mươi dòng cộng một con số.
const MAX_NOTES: usize = 20;

/// Một tài liệu như tầng trên thấy nó. Chuyển sang `DocumentView` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Document {
    pub id: String,
    /// Tệp thật trong thư mục dự án. Đây là chỗ người dùng mở được bằng Finder.
    pub path: PathBuf,
    /// Chỗ tệp đến từ đó. Bằng `path` với tệp vốn đã nằm trong thư mục dự án; khác `path`
    /// khi người dùng kéo nó vào từ ngoài, và khi đó nó là câu trả lời cho "tệp này ở đâu
    /// ra".
    pub origin: String,
    pub title: String,
    pub format: Format,
    pub bytes: u64,
    pub chunks: u32,
    pub embedded: bool,
    pub added_at: i64,
    /// `None` cộng `embedded == false` nghĩa là **đang xếp hàng**, không phải hỏng.
    pub error: Option<String>,
}

impl From<DocumentRow> for Document {
    fn from(row: DocumentRow) -> Document {
        Document {
            id: row.id,
            path: row.path,
            origin: row.origin,
            title: row.title,
            format: row.format,
            bytes: row.bytes,
            chunks: row.chunks,
            embedded: row.embedded,
            added_at: row.added_at,
            error: row.error,
        }
    }
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
    /// Số tệp đọc được mà lần quét gần nhất nhìn thấy trong thư mục.
    pub files_seen: u32,
    /// Số tệp lần quét gần nhất bỏ qua vì chạm trần — kích thước hoặc [`MAX_FILES`].
    pub files_skipped: u32,
    /// Số tệp đã thử đọc và không đọc được.
    pub unreadable: u32,
    /// Số tệp còn trong thư mục nhưng người dùng đã bỏ khỏi thư viện.
    pub excluded: u32,
    /// Lần quét gần nhất, millis. `None` là chưa quét lần nào.
    pub scanned_at: Option<i64>,
    /// Có lượt quét nào đang chạy không, và tới đâu rồi.
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
}

/// Giai đoạn của một tệp trong lúc nạp. Chuyển sang `IngestProgress.stage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestStage {
    /// Bắt đầu một tệp: rút chữ, cắt đoạn, nhúng.
    Reading,
    Stored,
    Failed,
    /// Tệp bị bỏ qua **có lý do**: quá lớn, hoặc nằm ngoài trần số tệp. Tách khỏi
    /// `Failed` vì đây không phải một tệp hỏng — nó lành lặn, thư viện mới là bên từ chối.
    Skipped,
    /// Tệp đã biến mất khỏi thư mục nên hàng của nó vừa rời thư viện.
    Removed,
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

/// Cái mà tool và tầng trên nhìn thấy. Tách khỏi [`Library`] để bài kiểm chứng và phía
/// `app/` cắm được một bản khác mà không phải mở một cơ sở dữ liệu thật.
///
/// # Vì sao `sync`, `ingest` và `remove` nằm ở đây chứ không chỉ trên [`Library`]
///
/// Chúng là **lệnh của giao diện**, không phải tool của mô hình — không có tool nào nạp
/// hay xoá tài liệu, và đó là có chủ ý (xem [`crate::tools`]). Nhưng chúng vẫn phải nằm
/// trên seam, vì thiếu chúng thì tầng trên chỉ cầm được một `Arc<dyn DocLibrary>` và
/// buộc phải mở một [`Library`] **thứ hai** trên cùng thư mục để quét.
///
/// Hai handle trên cùng một tệp SQLite thì WAL chịu được. Hai handle với **hai bộ nhúng
/// khác nhau** thì không: bản thứ hai sẽ ghi vector của một mô hình khác vào cùng bảng
/// `vectors`, và cosine giữa hai không gian nhúng khác nhau là một con số vô nghĩa trông
/// y hệt một con số có nghĩa. Không có gì trong hệ kiểu ngăn được chuyện đó — trừ việc
/// không để ai có lý do mở handle thứ hai.
#[async_trait]
pub trait DocLibrary: Send + Sync + 'static {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError>;
    fn documents(&self) -> Result<Vec<Document>, RagError>;
    /// Đọc liền mạch một tài liệu theo đoạn.
    fn chunks(&self, document_id: &str, offset: usize, limit: usize) -> Result<Vec<Hit>, RagError>;
    fn stats(&self) -> Result<Stats, RagError>;
    /// Bắt kịp thư mục dự án. Đây là đường vào chính của một dự án tài liệu — gọi lúc mở
    /// dự án, và gọi lại khi người dùng muốn làm mới.
    fn sync(&self) -> BoxStream<'_, IngestEvent>;
    /// Đưa những tệp này về thư mục dự án rồi nạp. Dòng mượn `&self`, nên nó không sống
    /// lâu hơn thư viện — một dòng còn chạy sau khi kho đã đóng là một dòng ghi vào chỗ
    /// trống.
    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent>;
    /// Bỏ một tài liệu khỏi thư viện. **Không** xoá tệp trên đĩa — xem [`Library::remove`].
    fn remove(&self, id: &str) -> Result<(), RagError>;
}

/// Seam thư viện tài liệu.
pub enum Docs {}
impl ServiceKey for Docs {
    type Api = dyn DocLibrary;
    const NAME: &'static str = "rag.docs";
}

pub struct Library {
    /// Thư mục tài liệu của người dùng. Thư viện là chính nó.
    root: PathBuf,
    store: Arc<Store>,
    embedder: Option<Arc<dyn Embedder>>,
    opts: ChunkOpts,
    /// Trần số tệp một lần quét nạp. Trên [`Library`] chứ không phải một hằng cứng để bài
    /// kiểm chứng hạ nó xuống được mà không phải sinh ra năm nghìn tệp thật.
    limit: usize,
    progress: Progress,
    extracts: AtomicU64,
}

impl Library {
    /// `dir` là thư mục kho — cơ sở dữ liệu nằm trong đó. `root` là **thư mục tài liệu
    /// của người dùng**, và nó là thứ được quét.
    ///
    /// Hai đường dẫn chứ không một, vì hai thứ có vòng đời khác nhau: kho dựng lại được
    /// bất cứ lúc nào từ `root`, còn `root` là dữ liệu của người dùng và không được sinh
    /// thêm gì trong đó ngoài chính tệp họ kéo vào.
    pub fn open(
        dir: &Path,
        root: &Path,
        embedder: Option<Arc<dyn Embedder>>,
    ) -> Result<Library, RagError> {
        std::fs::create_dir_all(dir).map_err(|err| RagError::io(dir.display(), err))?;
        // Phân giải gốc **trước** khi đi, đúng như `pai-index`: đường lưu trong kho phải
        // trùng từng byte với đường mà lần quét sau sinh ra, nếu không mỗi lần quét lại
        // thấy toàn tệp lạ và nạp lại cả thư mục. Trên macOS, `/var` với `/private/var` là
        // đúng cặp chuỗi đó.
        //
        // Thư mục chưa tồn tại thì **không** tạo: một đường dẫn gõ nhầm hay một ổ chưa cắm
        // phải hiện ra như thư viện rỗng có lời giải thích, chứ không phải như một thư mục
        // mới tinh do ứng dụng lặng lẽ dựng lên ở chỗ người dùng không định.
        let root = match root.canonicalize() {
            Ok(found) => found,
            Err(err) => {
                tracing::debug!(root = %root.display(), %err, "chưa phân giải được thư mục dự án");
                root.to_path_buf()
            }
        };
        let store = Store::open(&dir.join(DB_NAME), &root)?;
        let rebuilt = store.opened() == Opened::Rebuilt;
        let library = Library {
            root,
            store: Arc::new(store),
            embedder,
            opts: ChunkOpts::default(),
            limit: MAX_FILES,
            progress: Progress::default(),
            extracts: AtomicU64::new(0),
        };
        if rebuilt {
            library.rebuild_from_root()?;
        }
        library.sync_embedder_identity()?;
        Ok(library)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Số lần một tệp thật sự đi qua bộ rút chữ kể từ lúc mở.
    ///
    /// Số này là cách duy nhất nhìn thấy được rằng thư viện đang tăng dần chứ không đang
    /// âm thầm nạp lại cả thư mục: hai lần [`Library::sync`] liên tiếp trên một thư mục
    /// không đổi phải để nó nguyên. Bài kiểm chứng soi vào đây, và log cũng vậy — cùng vai
    /// với `CodeIndex::parse_count`.
    pub fn extract_count(&self) -> u64 {
        self.extracts.load(Ordering::Relaxed)
    }

    /// So bộ nhúng đang cắm với bộ nhúng đã sinh ra những vector trong kho, và xoá sạch
    /// vector khi chúng khác nhau.
    ///
    /// # Vì sao việc này phải xảy ra lúc mở, và phải xảy ra tự động
    ///
    /// Phía `app/` nối bộ nhúng theo **provider đang hoạt động**, nên chỉ cần người dùng
    /// đổi từ Ollama sang OpenAI là mô hình nhúng đổi theo — `nomic-embed-text` 768 chiều
    /// thành `text-embedding-3-small` 1536 chiều. Đây là một đường đi thường xuyên, không
    /// phải một tình huống hiếm.
    ///
    /// Và nó là loại hỏng tệ nhất mà crate này có thể mắc: cosine giữa hai không gian
    /// nhúng khác nhau vẫn trả về một số trong `[-1, 1]`, vẫn xếp hạng được, vẫn hiện lên
    /// giao diện như một kết quả. Không có gì báo lỗi. `semantic_ready` nói *sẵn sàng*
    /// trong khi thứ trả về là rác — đúng cái mà cả [`Stats::reason`] sinh ra để tránh.
    ///
    /// Hai phép so, và chúng không đối xứng:
    ///
    /// - **Tên khác nhau ⇒ xoá.** Đây là tín hiệu chính.
    /// - **Số chiều khác nhau ⇒ xoá**, nhưng chỉ khi *cả hai* bên đều biết số chiều của
    ///   mình. [`Embedder::dim`] được phép trả `None`, và coi "không biết" là "đã đổi" sẽ
    ///   xoá sạch vector mỗi lần mở chỉ vì cấu hình quên khai một con số.
    ///
    /// Chưa từng ghi danh tính thì không xoá gì cả: không có gì để so, và một thư viện
    /// nạp lúc chưa có bộ nhúng thì vốn dĩ chưa có vector nào.
    fn sync_embedder_identity(&self) -> Result<bool, RagError> {
        let Some(embedder) = &self.embedder else {
            // Gỡ bộ nhúng ra không làm vector cũ sai — nó chỉ làm chúng tạm thời không
            // dùng tới. Xoá ở đây là bắt người dùng nhúng lại cả thư viện chỉ vì họ tắt
            // Ollama đi một lát.
            return Ok(false);
        };
        let id = embedder.id().to_string();
        let dim = embedder.dim();

        let truoc_id = self.store.meta(store::META_EMBEDDER_ID)?;
        let truoc_dim = self
            .store
            .meta(store::META_EMBEDDER_DIM)?
            .and_then(|value| value.parse::<usize>().ok());

        let doi = match &truoc_id {
            None => false,
            Some(truoc) if *truoc != id => true,
            Some(_) => matches!((truoc_dim, dim), (Some(a), Some(b)) if a != b),
        };

        if doi {
            let xoa = self.store.forget_vectors()?;
            // Lý do hỏng của bộ nhúng cũ nói về một máy chủ đã bị thay; giữ lại thì
            // `stats()` đổ lỗi cho đúng thứ không còn liên quan.
            self.store.clear_errors()?;
            let truoc = truoc_id.clone().unwrap_or_default();
            self.store.set_meta(store::META_EMBEDDER_PREVIOUS, &truoc)?;
            tracing::info!(
                truoc = %truoc,
                sau = %id,
                vector_da_xoa = xoa,
                "đổi mô hình nhúng: xoá vector cũ, giữ nguyên tài liệu và đoạn"
            );
        }

        self.store.set_meta(store::META_EMBEDDER_ID, &id)?;
        // Không xoá con số đã biết chỉ vì lần này cấu hình không khai: một `dim()` trả
        // `None` là "tôi không biết", không phải "không có chiều nào".
        if let Some(dim) = dim {
            self.store
                .set_meta(store::META_EMBEDDER_DIM, &dim.to_string())?;
        }
        Ok(doi)
    }

    /// Đổi cách cắt đoạn. Có để bài kiểm chứng chạy được với văn bản ngắn.
    pub fn with_chunking(mut self, opts: ChunkOpts) -> Library {
        self.opts = opts;
        self
    }

    /// Hạ trần số tệp một lần quét nạp. Có để bài kiểm chứng chạm được vào đường chạm
    /// trần — một trần chỉ được kiểm bằng cách vượt qua nó.
    pub fn with_scan_limit(mut self, limit: usize) -> Library {
        self.limit = limit.max(1);
        self
    }

    /// Dựng lại toàn bộ từ thư mục dự án sau khi schema bị thay.
    ///
    /// Chạy đồng bộ ngay trong `open`: một thư viện nửa dựng là một thư viện trả về kết
    /// quả thiếu mà không nói gì, và người dùng sẽ kết luận rằng tài liệu của họ đã mất.
    /// Vector thì **không** dựng lại ở đây — nó cần mạng và cần `await`; đoạn vào FTS5 là
    /// đủ để tìm được ngay, còn phần ngữ nghĩa do [`Library::embed_pending`] dọn sau.
    ///
    /// Danh sách loại trừ không sống qua bước này: nó nằm trong chính kho vừa bị dựng lại.
    /// Người dùng sẽ thấy lại những tài liệu họ từng bỏ ra, và bỏ lại được — nhẹ hơn nhiều
    /// so với việc giữ một danh sách mà không còn gì đối chiếu.
    fn rebuild_from_root(&self) -> Result<(), RagError> {
        let plan = plan_scan(&self.store, &self.root, self.limit)?;
        let mut restored = 0usize;
        for (path, origin) in &plan.work {
            match self.absorb(path, origin.clone()) {
                Ok(_) => restored += 1,
                // Một tệp không dựng lại được không được chặn những tệp còn lại — đúng
                // cùng lý do với việc nạp lần đầu.
                Err(err) => tracing::warn!(path = %path.display(), %err, "bỏ qua khi dựng lại"),
            }
        }
        tracing::info!(
            restored,
            "đã dựng lại thư viện bằng một lần quét thư mục dự án"
        );
        Ok(())
    }

    /// Quét thư mục dự án và bắt kịp đĩa.
    ///
    /// Tăng dần theo `mtime` + kích thước: tệp không đổi thì **không** đi qua bộ rút chữ,
    /// không bị cắt đoạn lại, không bị nhúng lại. Tệp đã biến mất khỏi thư mục thì hàng
    /// của nó rời khỏi kho, kèm đoạn, hàng FTS và vector.
    pub fn sync(&self) -> BoxStream<'_, IngestEvent> {
        self.run(Source::Scan)
    }

    /// Đưa những tệp này về thư mục dự án rồi nạp.
    ///
    /// Tệp **đã nằm trong** thư mục dự án — kể cả trong thư mục con — thì nạp tại chỗ,
    /// không chép. Chép là nhân đôi dung lượng ngay trong thư mục người dùng đang nhìn, và
    /// từ lúc đó có hai tệp cùng nội dung mà chỉ một cái nhận được sửa đổi của họ.
    ///
    /// Tệp nằm ngoài thì được chép **vào chính thư mục dự án**, không vào một kho ẩn: lần
    /// quét sau nhặt nó lên như mọi tệp khác, và người dùng mở Finder ra thấy tệp mình vừa
    /// thêm nằm đúng chỗ họ mong đợi.
    pub fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        self.run(Source::Paths(paths))
    }

    /// Máy trạng thái chung của cả hai đường vào.
    ///
    /// Tường minh thay vì một kênh và một task nền: dòng này mượn `&self`, nên nó không
    /// được sống lâu hơn thư viện — và một task nền cầm bản sao `Arc` thì sống lâu hơn,
    /// rồi ghi tiếp vào một kho mà người dùng tưởng đã đóng.
    fn run(&self, source: Source) -> BoxStream<'_, IngestEvent> {
        let state = Cursor {
            library: self,
            source: Some(source),
            plan: Plan::default(),
            note_at: 0,
            total: 0,
            at: 0,
            announced: false,
            closed: false,
        };
        futures::stream::unfold(state, |mut state| async move {
            if let Some(source) = state.source.take() {
                // Đi thư mục, chép tệp và ghi SQLite đều là việc chặn; bước này ra khỏi
                // runtime trước khi sự kiện đầu tiên được phát.
                state.plan = state.library.prepare(source).await;
                state.total = state.plan.work.len() as u32;
                state.library.progress.start(state.total);
            }
            if state.note_at < state.plan.notes.len() {
                let mut event = state.plan.notes[state.note_at].clone();
                state.note_at += 1;
                // Mẫu số có ngay từ sự kiện đầu, kể cả khi sự kiện đầu là một lời từ chối.
                event.done = 0;
                event.total = state.total;
                return Some((event, state));
            }
            if state.at < state.plan.work.len() {
                let path = state.plan.work[state.at].0.display().to_string();
                if !state.announced {
                    // Báo trước khi làm, không phải sau: một sự kiện phát ra sau khi tệp
                    // đã xong thì thanh tiến trình luôn đứng yên đúng lúc nó cần động.
                    state.announced = true;
                    return Some((
                        IngestEvent {
                            path,
                            stage: IngestStage::Reading,
                            done: state.at as u32,
                            total: state.total,
                            finished: false,
                            error: None,
                            document: None,
                        },
                        state,
                    ));
                }
                let (source, origin) = state.plan.work[state.at].clone();
                let outcome = state.library.ingest_one(&source, origin).await;
                state.at += 1;
                state.announced = false;
                let done = state.at as u32;
                state.library.progress.step(done);
                let event = match outcome {
                    Ok(document) => IngestEvent {
                        path,
                        stage: IngestStage::Stored,
                        done,
                        total: state.total,
                        finished: false,
                        error: document.error.clone(),
                        document: Some(document),
                    },
                    Err(err) => IngestEvent {
                        path,
                        stage: IngestStage::Failed,
                        done,
                        total: state.total,
                        finished: false,
                        error: Some(err.to_string()),
                        document: None,
                    },
                };
                return Some((event, state));
            }
            if state.closed {
                return None;
            }
            state.closed = true;
            Some((
                IngestEvent {
                    path: String::new(),
                    stage: IngestStage::Finished,
                    done: state.total,
                    total: state.total,
                    finished: true,
                    error: None,
                    document: None,
                },
                state,
            ))
        })
        .boxed()
    }

    /// Bước chặn đầu tiên: dựng danh sách việc, và dọn những gì đã biến mất.
    async fn prepare(&self, source: Source) -> Plan {
        let store = self.store.clone();
        let root = self.root.clone();
        let limit = self.limit;
        let shown = self.root.display().to_string();
        let done = blocking(move || match source {
            Source::Paths(paths) => Ok(plan_paths(&store, &root, paths)),
            Source::Scan => plan_scan(&store, &root, limit),
        })
        .await;
        match done {
            Ok(plan) => plan,
            // Kho không mở được thì không có việc nào chạy, nhưng dòng vẫn phải nói ra —
            // một dòng kết thúc ngay ở `Finished` trông y hệt một thư mục rỗng.
            Err(err) => Plan {
                work: Vec::new(),
                notes: vec![note(&shown, IngestStage::Failed, Some(err.to_string()))],
            },
        }
    }

    async fn ingest_one(
        &self,
        source: &Path,
        origin: Option<String>,
    ) -> Result<Document, RagError> {
        let id = {
            let store = self.store.clone();
            let opts = self.opts;
            let source = source.to_path_buf();
            self.extracts.fetch_add(1, Ordering::Relaxed);
            // Đọc tệp, rút chữ PDF và ghi SQLite đều là việc chặn, và một PDF lớn chặn
            // hàng giây. Ra khỏi runtime, nếu không cả giao diện đứng trong lúc nạp.
            blocking(move || absorb_into(&store, opts, &source, origin)).await?
        };

        // Từ đây trở đi mọi thất bại đều được **ghi vào hàng tài liệu**, không ném lên:
        // tài liệu đã có trong FTS5 rồi, và biến nó thành một lần nạp hỏng là vứt đi công
        // đã làm cùng với khả năng tìm bằng từ khoá.
        let note = match self.embed_pending().await {
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(%err, "nạp xong nhưng chưa nhúng được vector");
                Some(err.to_string())
            }
        };
        self.store.set_error(&id, note.as_deref())?;
        self.document(&id)
    }

    /// Nhúng mọi đoạn còn thiếu vector. Không có bộ nhúng thì không có gì để làm.
    ///
    /// Gọi được nhiều lần và gọi lúc nào cũng được: đây là đường mà một thư viện đã nạp
    /// lúc Ollama tắt đi theo để bắt kịp khi Ollama bật lại.
    pub async fn embed_pending(&self) -> Result<usize, RagError> {
        let Some(embedder) = self.embedder.clone() else {
            return Ok(0);
        };
        let mut total = 0usize;
        loop {
            let store = self.store.clone();
            let batch = blocking(move || store.chunks_without_vectors(MAX_BATCH)).await?;
            if batch.is_empty() {
                // Không còn gì xếp hàng nghĩa là đợt nhúng lại đã xong. Xoá dấu ở đây chứ
                // không giữ một cờ trong bộ nhớ: trạng thái này phải sống qua lần khởi
                // động lại, vì nhúng lại một thư viện lớn không xong trong một phiên.
                let store = self.store.clone();
                let key = store::META_EMBEDDER_PREVIOUS;
                blocking(move || store.clear_meta(key)).await?;
                return Ok(total);
            }
            let texts: Vec<String> = batch.iter().map(|(_, body)| body.clone()).collect();
            let vectors = embedder.embed(&texts).await?;
            let rows: Vec<(i64, Vec<f32>)> = batch.iter().map(|(id, _)| *id).zip(vectors).collect();
            total += rows.len();
            let store = self.store.clone();
            blocking(move || store.put_vectors(&rows)).await?;
        }
    }

    pub fn documents(&self) -> Result<Vec<Document>, RagError> {
        Ok(self
            .store
            .documents()?
            .into_iter()
            .map(Document::from)
            .collect())
    }

    fn document(&self, id: &str) -> Result<Document, RagError> {
        self.store
            .documents()?
            .into_iter()
            .find(|row| row.id == id)
            .map(Document::from)
            .ok_or_else(|| RagError::NotFound(id.to_string()))
    }

    /// Bỏ một tài liệu khỏi thư viện. **Tệp trên đĩa không bị đụng tới.**
    ///
    /// # Vì sao đây không phải là chỗ xoá tệp
    ///
    /// Bản trước xoá một bản sao trong kho ẩn, và việc đó vô hại vì bản sao là của thư
    /// viện. Giờ `path` trỏ vào **tệp thật của người dùng**, và cùng một dòng lệnh trở
    /// thành một hành động không lấy lại được: một thư viện tự xoá tệp của người dùng thì
    /// sau một lần là không lấy lại được niềm tin, dù nó có hỏi trước hay không.
    /// `pai-project::forget` bỏ một dự án khỏi danh sách mà không đụng tới thư mục, và đây
    /// là đúng cái luật ấy. Nếu có ngày cần một đường xoá tệp thật thì nó phải là một hàm
    /// **riêng, tên khác**, để không ai gọi nhầm nó khi định làm việc này.
    ///
    /// # Và vì sao có danh sách loại trừ
    ///
    /// Tệp vẫn nằm trong thư mục, nên lần [`Library::sync`] ngay sau đó sẽ nhặt nó lên lại
    /// — một nút bấm không có tác dụng, tệ hơn cả một nút bấm không tồn tại. Nên đường dẫn
    /// được ghi vào bảng `excluded` và lần quét bỏ qua nó. Người dùng lấy nó lại bằng cách
    /// tự tay nạp lại đúng tệp đó qua [`Library::ingest`], vì một lời nói sau đè lên một
    /// lời nói trước. [`Stats::excluded`] đếm số tệp đang ở trạng thái này, để giao diện
    /// nói được "thư mục có 12 tệp, thư viện có 11" thay vì để người dùng tự đoán.
    pub fn remove(&self, id: &str) -> Result<(), RagError> {
        let doc = self.document(id)?;
        self.store.remove_document(id)?;
        let path = doc.path.display().to_string();
        self.store.exclude(&path, now_millis())?;
        // Lý do hỏng cũng đi theo: giữ lại thì lần quét sau vẫn đếm tệp này vào số "không
        // đọc được" trong khi nó đã không còn thuộc thư viện.
        self.store.clear_failure(&path)?;
        Ok(())
    }

    pub fn stats(&self) -> Result<Stats, RagError> {
        let counts = self.store.counts()?;
        let embedder = self.embedder.as_ref().map(|item| item.id().to_string());
        let failure = self.store.first_error()?;
        let (unreadable, excluded) = self.store.side_counts()?;
        let files_seen = self.meta_number(store::META_SCAN_FILES).unwrap_or(0) as u32;
        let files_skipped = self.meta_number(store::META_SCAN_SKIPPED).unwrap_or(0) as u32;
        let scanned_at = self.meta_number(store::META_SCAN_AT);
        // Bộ nhúng cũ, nếu đợt nhúng lại chưa xong. Đọc từ kho chứ không từ một cờ trong
        // bộ nhớ: nhúng lại một thư viện lớn không xong trong một phiên, và người dùng
        // mở lại ứng dụng thì vẫn phải đọc được lời giải thích.
        let doi_mo_hinh = self.store.meta(store::META_EMBEDDER_PREVIOUS)?;

        let (semantic_ready, reason) = if counts.chunks == 0 {
            // Nhánh này đứng **trước** nhánh bộ nhúng, và thứ tự đó là cả nội dung của
            // đợt sửa này: người vừa chỉ vào một thư mục rồi thấy con số 0 đang hỏi "vì
            // sao không thấy tệp nào của tôi", không hỏi về mô hình nhúng.
            (
                false,
                Some(self.empty_reason(files_skipped, unreadable, excluded)),
            )
        } else if embedder.is_none() {
            (
                false,
                Some(
                    "Chưa cấu hình mô hình nhúng, nên tìm kiếm đang chạy bằng từ khoá. \
                     Chọn một mô hình nhúng trong phần Provider để bật tìm theo ý nghĩa."
                        .to_string(),
                ),
            )
        } else if let Some(err) = failure {
            (
                false,
                Some(format!(
                    "Bộ nhúng chưa trả lời nên còn {} trong {} đoạn chưa có vector; \
                     tìm kiếm vẫn chạy bằng từ khoá. Lý do gần nhất: {err}",
                    counts.chunks - counts.embedded_chunks,
                    counts.chunks
                )),
            )
        } else if let Some(truoc) = doi_mo_hinh.filter(|_| counts.embedded_chunks < counts.chunks) {
            // Người dùng nhìn thấy số đoạn đã nhúng tụt về 0 và sẽ tưởng thư viện hỏng.
            // Câu này là toàn bộ khác biệt giữa "đang làm việc" và "vừa mất dữ liệu".
            let tu = match truoc.is_empty() {
                // Danh tính cũ rỗng chỉ xảy ra với kho ghi bởi một bản trước khi có bảng
                // `meta`; vẫn nói ra chuyện đổi mô hình, chỉ không gọi được tên cũ.
                true => String::new(),
                false => format!(" từ `{truoc}`"),
            };
            (
                false,
                Some(format!(
                    "Mô hình nhúng vừa đổi{tu} sang `{}`, nên toàn bộ vector cũ đã bị bỏ và \
                     thư viện đang nhúng lại: {}/{} đoạn đã xong. Không có tài liệu nào bị \
                     mất, và tìm bằng từ khoá vẫn chạy bình thường trong lúc chờ.",
                    embedder.as_deref().unwrap_or("?"),
                    counts.embedded_chunks,
                    counts.chunks
                )),
            )
        } else if counts.embedded_chunks < counts.chunks {
            (
                false,
                Some(format!(
                    "Còn {} trong {} đoạn đang xếp hàng chờ nhúng; tìm theo ý nghĩa sẽ đầy \
                     đủ khi xong.",
                    counts.chunks - counts.embedded_chunks,
                    counts.chunks
                )),
            )
        } else {
            (true, None)
        };

        Ok(Stats {
            documents: counts.documents,
            chunks: counts.chunks,
            embedded_chunks: counts.embedded_chunks,
            embedder,
            semantic_ready,
            reason,
            root: self.root.clone(),
            files_seen,
            files_skipped,
            unreadable,
            excluded,
            scanned_at,
            scanning: self.progress.read(),
        })
    }

    /// Câu trả lời cho "tại sao chọn thư mục mà không thấy tệp nào".
    ///
    /// Nó phải gọi tên **thư mục cụ thể** và nói ra từng con số: một câu chung chung kiểu
    /// "thư viện trống" để người dùng lại đúng chỗ họ đang đứng.
    fn empty_reason(&self, skipped: u32, unreadable: u32, excluded: u32) -> String {
        let root = self.root.display();
        if !self.root.is_dir() {
            return format!(
                "Không mở được thư mục dự án {root}. Thư viện đang trống vì chưa đọc được \
                 tệp nào ở đó — hãy kiểm tra thư mục còn nằm đúng chỗ và ổ đĩa đã được nối."
            );
        }
        let mut cau = format!(
            "Thư mục dự án {root} chưa có tệp nào thư viện đọc được. Thư viện nhận văn \
             bản, markdown, CSV, HTML, PDF, DOCX và mã nguồn; ảnh và tệp nhị phân thì \
             không, và tệp ẩn cùng tệp bị `.gitignore` loại ra cũng không được quét."
        );
        if skipped > 0 {
            cau.push_str(&format!(
                " {skipped} tệp đã bị bỏ qua vì quá lớn hoặc vì thư mục vượt trần {MAX_FILES} tệp."
            ));
        }
        if unreadable > 0 {
            cau.push_str(&format!(" {unreadable} tệp đọc không ra chữ."));
        }
        if excluded > 0 {
            cau.push_str(&format!(
                " {excluded} tệp còn trong thư mục nhưng đã được bỏ khỏi thư viện."
            ));
        }
        cau
    }

    fn meta_number(&self, key: &str) -> Option<i64> {
        self.store
            .meta(key)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i64>().ok())
    }

    /// Tìm lai ghép: BM25 của FTS5 hợp nhất với cosine bằng RRF — xem [`crate::search`].
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError> {
        let limit = limit.max(1);
        // Lấy sâu hơn `limit` ở mỗi bảng trước khi hợp nhất: một đoạn đứng hạng 15 ở cả
        // hai bảng đáng lên đầu, mà cắt ở `limit` thì nó không bao giờ vào tới phép hợp
        // nhất. Bốn lần là đủ rộng mà vẫn giữ phép quét cosine trong một lần đọc.
        let pool = (limit * 4).max(20);

        let store = self.store.clone();
        let text = query.to_string();
        let keyword = blocking(move || store.search_keyword(&text, pool)).await?;

        let semantic = self.semantic_ranking(query, pool).await;
        let ranked = fuse(&keyword, &semantic, limit);

        let ids: Vec<i64> = ranked.iter().map(|row| row.chunk_id).collect();
        let store = self.store.clone();
        let rows = blocking(move || store.chunks_by_id(&ids)).await?;

        Ok(ranked
            .iter()
            .filter_map(|row| {
                let found = rows.iter().find(|item| item.id == row.chunk_id)?;
                Some(hit_of(found, row.score, row.matched_by))
            })
            .collect())
    }

    /// Nửa ngữ nghĩa của lần tìm. Trả về danh sách rỗng khi không làm được — **không**
    /// trả lỗi: bộ nhúng tắt không được phép biến một lần tìm thành một lần hỏng.
    async fn semantic_ranking(&self, query: &str, pool: usize) -> Vec<i64> {
        let Some(embedder) = self.embedder.clone() else {
            return Vec::new();
        };
        let asked = vec![query.to_string()];
        let embedded = match embedder.embed(&asked).await {
            Ok(vectors) => vectors,
            Err(err) => {
                tracing::warn!(%err, "bỏ qua phần ngữ nghĩa của lần tìm này");
                return Vec::new();
            }
        };
        let Some(vector) = embedded.into_iter().next() else {
            return Vec::new();
        };
        let store = self.store.clone();
        match blocking(move || store.all_vectors()).await {
            Ok(vectors) => rank_by_cosine(&vector, &vectors, pool),
            Err(err) => {
                tracing::warn!(%err, "không đọc được bảng vector");
                Vec::new()
            }
        }
    }

    pub fn chunks(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Hit>, RagError> {
        let rows = self.store.chunks_of(document_id, offset, limit)?;
        if rows.is_empty() && self.document(document_id).is_err() {
            return Err(RagError::NotFound(document_id.to_string()));
        }
        // Điểm `0.0` vì đây là đọc tuần tự, không phải xếp hạng — một điểm bịa ra ở đây
        // sẽ được giao diện vẽ ra như thể nó có nghĩa.
        Ok(rows
            .iter()
            .map(|row| hit_of(row, 0.0, MatchedBy::Keyword))
            .collect())
    }

    /// Gộp WAL. Gọi lúc tháo plugin.
    pub fn checkpoint(&self) -> Result<(), RagError> {
        self.store.checkpoint()
    }

    /// Nạp một tệp đang nằm trong thư mục dự án — dùng cho việc dựng lại.
    fn absorb(&self, path: &Path, origin: Option<String>) -> Result<String, RagError> {
        self.extracts.fetch_add(1, Ordering::Relaxed);
        absorb_into(&self.store, self.opts, path, origin)
    }
}

#[async_trait]
impl DocLibrary for Library {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError> {
        Library::search(self, query, limit).await
    }

    fn documents(&self) -> Result<Vec<Document>, RagError> {
        Library::documents(self)
    }

    fn chunks(&self, document_id: &str, offset: usize, limit: usize) -> Result<Vec<Hit>, RagError> {
        Library::chunks(self, document_id, offset, limit)
    }

    fn stats(&self) -> Result<Stats, RagError> {
        Library::stats(self)
    }

    fn sync(&self) -> BoxStream<'_, IngestEvent> {
        Library::sync(self)
    }

    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        Library::ingest(self, paths)
    }

    fn remove(&self, id: &str) -> Result<(), RagError> {
        Library::remove(self, id)
    }
}

/// Nguồn việc của một lượt nạp.
enum Source {
    /// Người dùng chỉ tay vào những tệp này.
    Paths(Vec<PathBuf>),
    /// Đi hết thư mục dự án và bắt kịp đĩa.
    Scan,
}

/// Việc của một lượt nạp, dựng xong ở bước chặn đầu tiên.
#[derive(Default)]
struct Plan {
    /// Tệp phải rút chữ lần này, theo thứ tự, kèm chỗ nó đến từ đâu. `None` là "vốn ở
    /// trong thư mục dự án", và khi đó đường dẫn chính là câu trả lời đúng.
    work: Vec<(PathBuf, Option<String>)>,
    /// Những gì phải nói ra trước khi bắt tay vào việc: tệp bị bỏ qua, tệp vừa rời thư
    /// viện, thư mục không mở được.
    notes: Vec<IngestEvent>,
}

struct Cursor<'a> {
    library: &'a Library,
    source: Option<Source>,
    plan: Plan,
    note_at: usize,
    total: u32,
    at: usize,
    announced: bool,
    closed: bool,
}

impl Drop for Cursor<'_> {
    /// Dòng bị thả giữa chừng — cửa sổ đóng, người dùng đổi dự án — vẫn phải tắt cờ "đang
    /// quét". Không có chỗ này thì `stats()` báo một lượt quét chạy mãi không xong.
    fn drop(&mut self) {
        self.library.progress.finish();
    }
}

/// Tiến trình của lượt quét đang chạy.
///
/// Một lượt tại một thời điểm là giả định đúng ở đây: giao diện có một nút và một thanh
/// tiến trình. Hai lượt chồng nhau thì con số thuộc về lượt sau, và đó là con số người
/// dùng đang nhìn.
#[derive(Default)]
struct Progress {
    active: AtomicBool,
    done: AtomicU32,
    total: AtomicU32,
}

impl Progress {
    fn start(&self, total: u32) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);
    }

    fn step(&self, done: u32) {
        self.done.store(done, Ordering::Relaxed);
    }

    fn finish(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    fn read(&self) -> Option<Scanning> {
        if !self.active.load(Ordering::Relaxed) {
            return None;
        }
        Some(Scanning {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        })
    }
}

fn hit_of(row: &ChunkRow, score: f32, matched_by: MatchedBy) -> Hit {
    Hit {
        document_id: row.document_id.clone(),
        title: row.title.clone(),
        path: row.path.clone(),
        ordinal: row.ord,
        heading: row.heading.clone(),
        text: row.body.clone(),
        score,
        matched_by,
    }
}

fn note(path: &str, stage: IngestStage, error: Option<String>) -> IngestEvent {
    IngestEvent {
        path: path.to_string(),
        stage,
        done: 0,
        total: 0,
        finished: false,
        error,
        document: None,
    }
}

/// Đưa những tệp người dùng chỉ tay vào về thư mục dự án, rồi xếp chúng vào danh sách việc.
fn plan_paths(store: &Store, root: &Path, paths: Vec<PathBuf>) -> Plan {
    let mut plan = Plan::default();
    for source in paths {
        match bring_in(root, &source) {
            Ok((dest, origin)) => {
                let shown = dest.display().to_string();
                // Người dùng tự tay nạp lại một tệp họ từng bỏ ra: lời nói sau đè lên lời
                // nói trước, nếu không nút "Chọn tệp…" sẽ im lặng không làm gì.
                if let Err(err) = store.allow(&shown) {
                    tracing::debug!(%err, path = %shown, "không gỡ được khỏi danh sách loại trừ");
                }
                plan.work.push((dest, origin));
            }
            Err(err) => plan.notes.push(note(
                &source.display().to_string(),
                IngestStage::Failed,
                Some(err.to_string()),
            )),
        }
    }
    plan
}

/// Tệp đã nằm trong thư mục dự án thì trả về đúng nó; tệp ngoài thì chép vào.
///
/// Chép **vào thư mục dự án**, không vào một kho ẩn: người dùng vừa thêm một tài liệu, và
/// chỗ họ sẽ đi tìm nó là thư mục họ đã chọn. Trùng tên thì thêm hậu tố chứ không ghi đè —
/// tệp bị ghi đè ở đây là tệp của người khác, và một lần ghi đè im lặng là một lần mất dữ
/// liệu mà không ai biết đã mất cái gì.
/// Trả về đường dẫn trong thư mục dự án, kèm `origin` — chỗ tệp đến từ đó khi nó vừa được
/// chép vào, và `None` khi nó vốn đã nằm sẵn ở đó.
fn bring_in(root: &Path, source: &Path) -> Result<(PathBuf, Option<String>), RagError> {
    let shown = source.display().to_string();
    let meta = std::fs::metadata(source).map_err(|err| RagError::io(&shown, err))?;
    if !meta.is_file() {
        return Err(RagError::Unsupported(shown));
    }
    if let Some(inside) = inside_root(root, source) {
        return Ok((inside, None));
    }
    std::fs::create_dir_all(root).map_err(|err| RagError::io(root.display(), err))?;
    let dest = free_name(root, source);
    std::fs::copy(source, &dest).map_err(|err| RagError::io(dest.display(), err))?;
    Ok((dest, Some(shown)))
}

/// Đường dẫn đã phân giải của `path` nếu nó nằm trong `root`, kể cả trong thư mục con.
///
/// Phân giải trước khi so: `~/NCS/./bai.md`, một liên kết mềm, và `/var` với `/private/var`
/// trên macOS đều là cùng một tệp mang ba chuỗi khác nhau, và so chuỗi thô sẽ chép một tệp
/// đè lên chính nó.
fn inside_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let found = path.canonicalize().ok()?;
    found.starts_with(root).then_some(found)
}

/// Một cái tên chưa ai dùng trong `root`.
fn free_name(root: &Path, source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tai-lieu"));
    let dest = root.join(&name);
    if !dest.exists() {
        return dest;
    }
    let stem = name
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "tai-lieu".to_string());
    let ext = name
        .extension()
        .map(|ext| ext.to_string_lossy().to_string());
    for lan in 1..1_000u32 {
        let ten = match &ext {
            Some(ext) => format!("{stem}-{lan}.{ext}"),
            None => format!("{stem}-{lan}"),
        };
        let thu = root.join(ten);
        if !thu.exists() {
            return thu;
        }
    }
    // Nghìn tệp cùng tên là chuyện không xảy ra, nhưng vòng lặp phải có lối ra và lối ra
    // đó không được là ghi đè.
    root.join(format!("{stem}-{}", uuid::Uuid::now_v7()))
}

/// Đi thư mục dự án, so với kho, và dọn những gì đã biến mất.
fn plan_scan(store: &Store, root: &Path, limit: usize) -> Result<Plan, RagError> {
    let mut plan = Plan::default();
    let shown = root.display().to_string();

    if std::fs::read_dir(root).is_err() {
        // Thư mục không mở được — ổ chưa cắm, thư mục vừa bị đổi tên. **Không** dọn gì
        // cả: coi "không thấy tệp nào" là "mọi tệp đã bị xoá" sẽ quét sạch thư viện của
        // người dùng vì một sợi cáp lỏng.
        plan.notes.push(note(
            &shown,
            IngestStage::Skipped,
            Some(format!(
                "không mở được thư mục dự án {shown}; thư viện giữ nguyên những gì đã có"
            )),
        ));
        return Ok(plan);
    }

    let (mut seen, skipped, mut notes) = walk(root, limit);
    let excluded = store.excluded()?;
    // Tệp người dùng đã bỏ khỏi thư viện thì không nạp lại — xem `Library::remove`.
    seen.retain(|path, _| !excluded.contains(path));

    let known = store.known_files()?;
    let failed = store.failures()?;
    for (path, print) in &seen {
        if unchanged(known.get(path), print) || unchanged(failed.get(path), print) {
            continue;
        }
        plan.work.push((PathBuf::from(path), None));
    }

    let gone: Vec<String> = known
        .keys()
        .filter(|path| !seen.contains_key(*path))
        .cloned()
        .collect();
    let quen: Vec<String> = failed
        .keys()
        .filter(|path| !seen.contains_key(*path))
        .cloned()
        .collect();
    store.forget_paths(&gone)?;
    store.forget_failures(&quen)?;
    for path in gone.iter().take(MAX_NOTES) {
        notes.push(note(path, IngestStage::Removed, None));
    }
    if gone.len() > MAX_NOTES {
        notes.push(note(
            &shown,
            IngestStage::Removed,
            Some(format!(
                "và {} tài liệu khác đã rời thư viện vì tệp không còn trong thư mục",
                gone.len() - MAX_NOTES
            )),
        ));
    }

    store.set_meta(store::META_SCAN_FILES, &seen.len().to_string())?;
    store.set_meta(store::META_SCAN_SKIPPED, &skipped.to_string())?;
    store.set_meta(store::META_SCAN_AT, &now_millis().to_string())?;

    plan.notes.extend(notes);
    Ok(plan)
}

/// Dấu vân tay trên đĩa khớp với dấu vân tay đã ghi.
fn unchanged(known: Option<&FileState>, print: &FileState) -> bool {
    known.is_some_and(|state| state.mtime == print.mtime && state.size == print.size)
}

/// Đi thư mục dự án bằng crate `ignore`, và trả về những tệp thư viện đọc được.
///
/// `require_git(false)` là bắt buộc chứ không phải tiện tay: `.gitignore` phải có tác dụng
/// kể cả khi thư mục chưa `git init`, vì người dùng viết tệp đó để nói "đừng nhìn vào
/// đây", và đó là ý định chứ không phải một chi tiết của git. `pai-index` đã cắn đúng lỗi
/// này một lần.
///
/// Trả về: tệp thấy được (đã sắp theo đường dẫn), số tệp bị bỏ qua, và những lời cần nói.
fn walk(root: &Path, limit: usize) -> (BTreeMap<String, FileState>, usize, Vec<IngestEvent>) {
    let mut seen: BTreeMap<String, FileState> = BTreeMap::new();
    let mut notes: Vec<IngestEvent> = Vec::new();
    let mut qua_lon = 0usize;

    let mut builder = WalkBuilder::new(root);
    builder.require_git(false);
    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        // Hỏi định dạng **trước** khi mở tệp: một thư mục ảnh mười nghìn tệp không được
        // biến thành mười nghìn lần đọc rồi mười nghìn lần từ chối.
        if format_for(path).is_none() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.len() > crate::extract::MAX_FILE_BYTES {
            qua_lon += 1;
            if notes.len() < MAX_NOTES {
                notes.push(note(
                    &path.display().to_string(),
                    IngestStage::Skipped,
                    Some(
                        RagError::TooLarge {
                            path: path.display().to_string(),
                            bytes: meta.len(),
                            limit: crate::extract::MAX_FILE_BYTES,
                        }
                        .to_string(),
                    ),
                ));
            }
            continue;
        }
        seen.insert(
            path.display().to_string(),
            FileState {
                mtime: mtime_of(&meta),
                size: meta.len() as i64,
            },
        );
    }

    if qua_lon > MAX_NOTES {
        notes.push(note(
            &root.display().to_string(),
            IngestStage::Skipped,
            Some(format!(
                "và {} tệp khác bị bỏ qua vì vượt trần {} byte",
                qua_lon - MAX_NOTES,
                crate::extract::MAX_FILE_BYTES
            )),
        ));
    }

    let mut skipped = qua_lon;
    if seen.len() > limit {
        // Cắt theo thứ tự đường dẫn chứ không theo thứ tự đi thư mục: cùng một thư mục
        // phải cho cùng một tập tệp ở mọi lần quét, nếu không thư viện đổi nội dung mỗi
        // lần mở mà không ai chạm vào gì.
        let thua = seen.len() - limit;
        skipped += thua;
        let bo: Vec<String> = seen.keys().skip(limit).cloned().collect();
        for path in &bo {
            seen.remove(path);
        }
        notes.push(note(
            &root.display().to_string(),
            IngestStage::Skipped,
            Some(format!(
                "thư mục có nhiều hơn {limit} tệp đọc được nên {thua} tệp không được nạp. \
                 Thư viện nhận {limit} tệp đầu theo thứ tự đường dẫn; hãy chia bớt sang thư \
                 mục khác, hoặc dùng `.gitignore` để chọn phần cần nạp."
            )),
        ));
    }

    (seen, skipped, notes)
}

/// Rút chữ, cắt đoạn, ghi kho. Toàn bộ phần chặn của việc nạp một tệp.
///
/// Không chép gì cả: tệp **đã** nằm trong thư mục dự án khi tới được đây, và chép nó thêm
/// một lần nữa là nhân đôi dung lượng ngay trong thư mục người dùng đang nhìn.
///
/// `origin` là chỗ tệp đến từ đó. `None` nghĩa là nó vốn ở trong thư mục dự án, và khi đó
/// đường dẫn chính là câu trả lời đúng — bịa ra một chỗ khác nghe hợp lý hơn là nói dối.
fn absorb_into(
    store: &Store,
    opts: ChunkOpts,
    path: &Path,
    origin: Option<String>,
) -> Result<String, RagError> {
    let shown = path.display().to_string();
    let meta = std::fs::metadata(path).map_err(|err| RagError::io(&shown, err))?;
    if meta.len() > crate::extract::MAX_FILE_BYTES {
        return Err(RagError::TooLarge {
            path: shown,
            bytes: meta.len(),
            limit: crate::extract::MAX_FILE_BYTES,
        });
    }
    let mtime = mtime_of(&meta);

    let extracted = match extract(path) {
        Ok(found) => found,
        Err(err) => {
            // Ghi lại **cùng dấu vân tay**: không có dòng này thì mỗi lần quét lại đi rút
            // chữ lại đúng những tệp đã hỏng, và bất biến "thư mục không đổi thì không rút
            // chữ lại tệp nào" chỉ còn đúng với thư mục toàn tệp lành. Người dùng sửa tệp
            // là `mtime` đổi, và đó là lời mời thử lại.
            store.put_failure(&shown, mtime, meta.len() as i64, &err.to_string())?;
            return Err(err);
        }
    };
    store.clear_failure(&shown)?;

    let chunks = chunk(&extracted.text, opts);
    let id = store
        .by_path(&shown)?
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    store.put_document(
        &id,
        path,
        &origin.unwrap_or(shown),
        &extracted.title,
        extracted.format,
        meta.len(),
        mtime,
        now_millis(),
        &chunks,
    )?;
    Ok(id)
}

/// `mtime` tính bằng nanosecond kể từ epoch — cùng đơn vị, cùng phép so với `pai-index`.
fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_nanos() as i64)
        .unwrap_or_default()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

async fn blocking<T, F>(body: F) -> Result<T, RagError>
where
    F: FnOnce() -> Result<T, RagError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(body).await {
        Ok(result) => result,
        Err(err) => Err(RagError::Unavailable(err.to_string())),
    }
}
