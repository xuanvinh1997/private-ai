//! Thư viện tài liệu: nạp, liệt kê, xoá, tìm.
//!
//! # Hai quyết định sản phẩm, không phải hai tuỳ chọn kỹ thuật
//!
//! **1. Tệp được sao vào kho của dự án trước khi nạp.** Người dùng kéo một tệp từ
//! Downloads vào; tuần sau họ dọn Downloads. Một thư viện trỏ vào chỗ trống là một thư
//! viện chết: `docs.read` hỏng, việc dựng lại chỉ mục hỏng, và không có gì gợi ý cho họ
//! rằng nguyên nhân là một thư mục họ đã dọn từ lâu. Bản sao được đặt tên theo băm nội
//! dung, nên nạp lại cùng một tệp ghi đè đúng bản sao cũ thay vì sinh thêm một bản.
//!
//! **2. Nhúng vector là bước được phép hỏng.** Khi không có bộ nhúng, hoặc khi Ollama
//! chưa bật, tài liệu **vẫn** được rút chữ, cắt đoạn và đưa vào FTS5 — tìm bằng từ khoá
//! chạy ngay. Lý do nằm ở phía người dùng chứ không ở phía kiến trúc: họ vừa thả hai mươi
//! tệp vào cửa sổ, và "không có gì xảy ra" là câu trả lời tệ nhất có thể đưa cho họ. Cái
//! chưa xong được nói ra ở [`Stats::reason`], và [`Library::embed_pending`] dọn nốt khi
//! bộ nhúng quay lại.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use pai_core::ServiceKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::chunk::{ChunkOpts, chunk};
use crate::embed::{Embedder, MAX_BATCH};
use crate::error::RagError;
use crate::extract::{Format, extract};
use crate::search::{MatchedBy, fuse, rank_by_cosine};
use crate::store::{self, ChunkRow, DocumentRow, Opened, Store};

/// Tên tệp cơ sở dữ liệu bên trong thư mục dự án.
const DB_NAME: &str = "library.sqlite";
/// Thư mục chứa bản sao tệp.
const FILES_DIR: &str = "files";

/// Một tài liệu như tầng trên thấy nó. Chuyển sang `DocumentView` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Document {
    pub id: String,
    /// Bản sao trong kho của dự án, **không** phải chỗ người dùng lấy nó.
    pub path: PathBuf,
    /// Chỗ người dùng lấy nó, giữ lại để nói được "tệp này đến từ đâu".
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

/// Sức khoẻ của thư viện. Chuyển sang `LibraryStats` phía `app/` một-một.
#[derive(Clone, Debug)]
pub struct Stats {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
    pub embedder: Option<String>,
    pub semantic_ready: bool,
    /// Câu tiếng Việt giải thích khi `semantic_ready` là `false`. Đây là chỗ duy nhất
    /// người dùng biết được vì sao kết quả của họ chỉ có từ khoá.
    pub reason: Option<String>,
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
    /// Bắt đầu một tệp: sao vào kho, rút chữ, cắt đoạn, nhúng.
    Reading,
    Stored,
    Failed,
    /// Cả mẻ đã xong. Luôn là sự kiện cuối cùng của dòng.
    Finished,
}

impl IngestStage {
    pub fn as_str(self) -> &'static str {
        match self {
            IngestStage::Reading => "reading",
            IngestStage::Stored => "stored",
            IngestStage::Failed => "failed",
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
    /// danh sách sau mỗi tệp — với hai mươi tệp thì đó là hai mươi lần vẽ lại.
    pub document: Option<Document>,
}

/// Cái mà tool và tầng trên nhìn thấy. Tách khỏi [`Library`] để bài kiểm chứng và phía
/// `app/` cắm được một bản khác mà không phải mở một cơ sở dữ liệu thật.
///
/// # Vì sao `ingest` và `remove` nằm ở đây chứ không chỉ trên [`Library`]
///
/// Chúng là **lệnh của giao diện**, không phải tool của mô hình — không có tool nào nạp
/// hay xoá tài liệu, và đó là có chủ ý (xem [`crate::tools`]). Nhưng chúng vẫn phải nằm
/// trên seam, vì thiếu chúng thì tầng trên chỉ cầm được một `Arc<dyn DocLibrary>` và
/// buộc phải mở một [`Library`] **thứ hai** trên cùng thư mục để nạp.
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
    /// Sao tệp vào kho của dự án rồi nạp. Dòng mượn `&self`, nên nó không sống lâu hơn
    /// thư viện — một dòng còn chạy sau khi kho đã đóng là một dòng ghi vào chỗ trống.
    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent>;
    fn remove(&self, id: &str) -> Result<(), RagError>;
}

/// Seam thư viện tài liệu.
pub enum Docs {}
impl ServiceKey for Docs {
    type Api = dyn DocLibrary;
    const NAME: &'static str = "rag.docs";
}

pub struct Library {
    files_dir: PathBuf,
    store: Arc<Store>,
    embedder: Option<Arc<dyn Embedder>>,
    opts: ChunkOpts,
}

impl Library {
    pub fn open(dir: &Path, embedder: Option<Arc<dyn Embedder>>) -> Result<Library, RagError> {
        let files_dir = dir.join(FILES_DIR);
        std::fs::create_dir_all(&files_dir)
            .map_err(|err| RagError::io(files_dir.display(), err))?;
        let store = Store::open(&dir.join(DB_NAME), &files_dir)?;
        let rebuilt = store.opened() == Opened::Rebuilt;
        let library = Library {
            files_dir,
            store: Arc::new(store),
            embedder,
            opts: ChunkOpts::default(),
        };
        if rebuilt {
            library.rebuild_from_store()?;
        }
        library.sync_embedder_identity()?;
        Ok(library)
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

    /// Dựng lại toàn bộ từ bản sao tệp sau khi schema bị thay.
    ///
    /// Chạy đồng bộ ngay trong `open`: một thư viện nửa dựng là một thư viện trả về kết
    /// quả thiếu mà không nói gì, và người dùng sẽ kết luận rằng tài liệu của họ đã mất.
    /// Vector thì **không** dựng lại ở đây — nó cần mạng và cần `await`; đoạn vào FTS5 là
    /// đủ để tìm được ngay, còn phần ngữ nghĩa do [`Library::embed_pending`] dọn sau.
    fn rebuild_from_store(&self) -> Result<(), RagError> {
        let entries = std::fs::read_dir(&self.files_dir)
            .map_err(|err| RagError::io(self.files_dir.display(), err))?;
        let mut restored = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            match self.absorb(&path, &path.display().to_string()) {
                Ok(_) => restored += 1,
                // Một tệp không dựng lại được không được chặn những tệp còn lại — đúng
                // cùng lý do với việc nạp lần đầu.
                Err(err) => tracing::warn!(path = %path.display(), %err, "bỏ qua khi dựng lại"),
            }
        }
        tracing::info!(restored, "đã dựng lại thư viện từ bản sao tệp trong kho");
        Ok(())
    }

    /// Sao tệp vào kho rồi nạp. Dòng sự kiện để giao diện thấy tiến trình.
    pub fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        let total = paths.len() as u32;
        // Máy trạng thái tường minh thay vì một kênh và một task nền: dòng này mượn
        // `&self`, nên nó không được sống lâu hơn thư viện — và một task nền cầm bản sao
        // `Arc` thì sống lâu hơn, rồi ghi tiếp vào một kho mà người dùng tưởng đã đóng.
        let state = IngestCursor {
            library: self,
            paths,
            total,
            at: 0,
            announced: false,
            closed: false,
        };
        futures::stream::unfold(state, |mut state| async move {
            if state.at < state.paths.len() {
                let path = state.paths[state.at].display().to_string();
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
                let source = state.paths[state.at].clone();
                let outcome = state.library.ingest_one(&source).await;
                state.at += 1;
                state.announced = false;
                let done = state.at as u32;
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

    async fn ingest_one(&self, source: &Path) -> Result<Document, RagError> {
        let id = {
            let store = self.store.clone();
            let files_dir = self.files_dir.clone();
            let opts = self.opts;
            let source = source.to_path_buf();
            // Đọc tệp, rút chữ PDF và ghi SQLite đều là việc chặn, và một PDF lớn chặn
            // hàng giây. Ra khỏi runtime, nếu không cả giao diện đứng trong lúc nạp.
            blocking(move || absorb_into(&store, &files_dir, opts, &source, None)).await?
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

    /// Xoá một tài liệu, kể cả bản sao tệp.
    ///
    /// Bản sao phải đi theo: nó là nguồn của việc dựng lại chỉ mục, nên để nó ở lại nghĩa
    /// là tài liệu người dùng vừa xoá sẽ sống lại ở lần đổi schema kế tiếp.
    pub fn remove(&self, id: &str) -> Result<(), RagError> {
        let doc = self.document(id)?;
        self.store.remove_document(id)?;
        if doc.path.starts_with(&self.files_dir)
            && let Err(err) = std::fs::remove_file(&doc.path)
        {
            tracing::warn!(path = %doc.path.display(), %err, "xoá được tài liệu nhưng không xoá được bản sao");
        }
        Ok(())
    }

    pub fn stats(&self) -> Result<Stats, RagError> {
        let counts = self.store.counts()?;
        let embedder = self.embedder.as_ref().map(|item| item.id().to_string());
        let failure = self.store.first_error()?;
        // Bộ nhúng cũ, nếu đợt nhúng lại chưa xong. Đọc từ kho chứ không từ một cờ trong
        // bộ nhớ: nhúng lại một thư viện lớn không xong trong một phiên, và người dùng
        // mở lại ứng dụng thì vẫn phải đọc được lời giải thích.
        let doi_mo_hinh = self.store.meta(store::META_EMBEDDER_PREVIOUS)?;

        let (semantic_ready, reason) = if embedder.is_none() {
            (
                false,
                Some(
                    "Chưa cấu hình mô hình nhúng, nên tìm kiếm đang chạy bằng từ khoá. \
                     Chọn một mô hình nhúng trong phần Provider để bật tìm theo ý nghĩa."
                        .to_string(),
                ),
            )
        } else if counts.chunks == 0 {
            (
                false,
                Some("Thư viện chưa có tài liệu nào để tìm.".to_string()),
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
        })
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

    /// Nạp một tệp đã nằm sẵn trong kho — dùng cho việc dựng lại.
    fn absorb(&self, path: &Path, origin: &str) -> Result<String, RagError> {
        absorb_into(
            &self.store,
            &self.files_dir,
            self.opts,
            path,
            Some(origin.to_string()),
        )
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

    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        Library::ingest(self, paths)
    }

    fn remove(&self, id: &str) -> Result<(), RagError> {
        Library::remove(self, id)
    }
}

struct IngestCursor<'a> {
    library: &'a Library,
    paths: Vec<PathBuf>,
    total: u32,
    at: usize,
    announced: bool,
    closed: bool,
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

/// Sao tệp vào kho, rút chữ, cắt đoạn, ghi kho. Toàn bộ phần chặn của việc nạp.
///
/// `origin` là chỗ tệp đến từ đó. Khi dựng lại từ kho thì nó chính là bản sao — ta đã
/// không còn biết đường dẫn gốc, và bịa ra một đường dẫn nghe hợp lý hơn là nói dối.
fn absorb_into(
    store: &Store,
    files_dir: &Path,
    opts: ChunkOpts,
    source: &Path,
    origin: Option<String>,
) -> Result<String, RagError> {
    let shown = source.display().to_string();
    let bytes = {
        let meta = std::fs::metadata(source).map_err(|err| RagError::io(&shown, err))?;
        if meta.len() > crate::extract::MAX_FILE_BYTES {
            return Err(RagError::TooLarge {
                path: shown.clone(),
                bytes: meta.len(),
                limit: crate::extract::MAX_FILE_BYTES,
            });
        }
        meta.len()
    };

    let data = std::fs::read(source).map_err(|err| RagError::io(&shown, err))?;
    let sha = format!("{:x}", Sha256::digest(&data));

    // Tên bản sao suy từ băm nội dung: nạp lại cùng một tệp ghi đè đúng bản sao cũ, nên
    // kho không phình lên với những bản trùng mà không ai xoá được.
    let stored = files_dir.join(match source.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => format!("{}.{ext}", &sha[..16]),
        None => sha[..16].to_string(),
    });
    if stored != source {
        std::fs::write(&stored, &data).map_err(|err| RagError::io(stored.display(), err))?;
    }

    // Rút chữ từ **bản sao**, không từ tệp gốc: nếu hai bên khác nhau thì mọi thứ về sau
    // — `docs.read`, việc dựng lại — nói về bản sao, nên nó phải là thứ được đọc.
    let extracted = extract(&stored).map_err(|err| match err {
        // Lỗi nói về bản sao thì người dùng không nhận ra tệp nào của mình; đổi lại tên.
        RagError::Extract { reason, .. } => RagError::Extract {
            path: shown.clone(),
            reason,
        },
        RagError::Binary(_) => RagError::Binary(shown.clone()),
        RagError::Unsupported(_) => RagError::Unsupported(shown.clone()),
        other => other,
    })?;

    let chunks = chunk(&extracted.text, opts);
    let id = store
        .by_sha(&sha)?
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    store.put_document(
        &id,
        &stored,
        &origin.unwrap_or(shown),
        &extracted.title,
        extracted.format,
        &sha,
        bytes,
        now_millis(),
        &chunks,
    )?;
    Ok(id)
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
