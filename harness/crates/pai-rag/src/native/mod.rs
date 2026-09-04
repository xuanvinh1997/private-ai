mod config;
mod embedding;
mod extract;
mod rerank;
mod vector;
mod vision;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use pai_rag_core::{
    ChunkRow, DocumentInput, MatchedBy as CoreMatchedBy, SectionAwareSplitter, Store,
    embedding_text_for, fuse,
};
use parking_lot::Mutex;
use serde_json::json;
use sha1::{Digest, Sha1};

use self::{
    config::{ChunkConfig, NativeConfig, OcrConfig, ProviderConfig, RerankConfig, VectorConfig},
    embedding::{EMBED_INPUT_VERSION, EmbeddingClient, MAX_BATCH},
    extract::{EXTRACT_VERSION, ReaderKind, reader_for},
    vector::Qdrant,
    vision::VisionClient,
};
use crate::{
    DocLibrary, Document, Format, Hit, IngestEvent, IngestStage, MatchedBy, RagError, Scanning,
    Stats,
};

const MAX_FILES: usize = 5_000;

/// In-process Rust document library.
pub struct NativeLibrary {
    root: PathBuf,
    project: String,
    config_path: PathBuf,
    store: Mutex<Store>,
    runtime: Mutex<RuntimeState>,
    last_embed_error: Mutex<Option<String>>,
    scanning: Mutex<Option<Scanning>>,
}

struct RuntimeState {
    stamp: Option<std::time::SystemTime>,
    embedding_config: ProviderConfig,
    vision_config: ProviderConfig,
    vector_config: VectorConfig,
    chunk_config: ChunkConfig,
    ocr_config: OcrConfig,
    rerank_config: RerankConfig,
    embedder: Option<EmbeddingClient>,
    vision: Option<VisionClient>,
    vectors: Qdrant,
    splitter: SectionAwareSplitter,
    candidate_pool: usize,
}

#[derive(Clone)]
struct RuntimeSnapshot {
    embedder: Option<EmbeddingClient>,
    vision: Option<VisionClient>,
    vectors: Qdrant,
    splitter: SectionAwareSplitter,
    candidate_pool: usize,
    ocr: OcrConfig,
    rerank: RerankConfig,
}

/// Delete a closed project's native index. User documents are never inside this directory.
pub async fn purge_library(config_path: &Path, project: &str) -> Result<(), RagError> {
    let (directory, vector_config, collection) =
        NativeConfig::purge_parts(config_path, project.trim())?;
    let vectors = Qdrant::new(&vector_config, collection)?;
    if let Err(error) = vectors.drop_collection().await {
        // A dead optional vector service must not make a local project undeletable.
        tracing::warn!(%error, project, "could not purge Qdrant collection");
    }
    if directory.is_dir() {
        std::fs::remove_dir_all(&directory).map_err(|error| {
            RagError::Service(format!(
                "không xoá được thư viện `{}`: {error}",
                directory.display()
            ))
        })?;
    }
    Ok(())
}

impl NativeLibrary {
    pub fn open(config_path: PathBuf, project: String, root: PathBuf) -> Result<Self, RagError> {
        let native = NativeConfig::load(&config_path, &project, &root)?;
        let mut store = Store::open(&native.store_path).map_err(store_error)?;
        reconcile(&mut store, &native)?;
        let embedder = if native.embedding.model.trim().is_empty() {
            None
        } else {
            Some(EmbeddingClient::new(native.embedding.clone())?)
        };
        let vectors = Qdrant::new(&native.vectors, native.collection())?;
        let vision = if native.vision.model.trim().is_empty() {
            None
        } else {
            Some(VisionClient::new(native.vision.clone())?)
        };
        let runtime = RuntimeState {
            stamp: config_stamp(&config_path),
            embedding_config: native.embedding.clone(),
            vision_config: native.vision.clone(),
            vector_config: native.vectors.clone(),
            chunk_config: native.chunk.clone(),
            ocr_config: native.ocr.clone(),
            rerank_config: native.rerank.clone(),
            embedder,
            vision,
            vectors,
            splitter: SectionAwareSplitter::new(native.chunk.size, native.chunk.overlap),
            candidate_pool: native.rerank.candidates.max(20),
        };
        Ok(Self {
            root: native.root,
            project: native.project,
            config_path,
            store: Mutex::new(store),
            runtime: Mutex::new(runtime),
            last_embed_error: Mutex::new(None),
            scanning: Mutex::new(None),
        })
    }

    fn runtime(&self) -> Result<RuntimeSnapshot, RagError> {
        let stamp = config_stamp(&self.config_path);
        let mut state = self.runtime.lock();
        if stamp != state.stamp {
            let fresh = NativeConfig::load(&self.config_path, &self.project, &self.root)?;
            let store_path_changed = self
                .store
                .lock()
                .path()
                .is_some_and(|path| path != fresh.store_path);
            if store_path_changed || fresh.root != self.root {
                return Err(RagError::Service(
                    "đường dẫn dự án hoặc kho RAG đã đổi; mở lại dự án để chuyển an toàn".into(),
                ));
            }
            if fresh.embedding != state.embedding_config || fresh.vectors != state.vector_config {
                state.embedder = if fresh.embedding.model.trim().is_empty() {
                    None
                } else {
                    Some(EmbeddingClient::new(fresh.embedding.clone())?)
                };
                state.vectors = Qdrant::new(&fresh.vectors, fresh.collection())?;
                state.embedding_config = fresh.embedding.clone();
                state.vector_config = fresh.vectors.clone();
                self.store
                    .lock()
                    .set_identity(
                        &fresh.embedding.model,
                        fresh.embedding.dim,
                        EMBED_INPUT_VERSION,
                        EXTRACT_VERSION,
                    )
                    .map_err(store_error)?;
                *self.last_embed_error.lock() = None;
            }
            if fresh.vision != state.vision_config {
                state.vision = if fresh.vision.model.trim().is_empty() {
                    None
                } else {
                    Some(VisionClient::new(fresh.vision.clone())?)
                };
                state.vision_config = fresh.vision.clone();
            }
            if fresh.chunk != state.chunk_config {
                state.splitter = SectionAwareSplitter::new(fresh.chunk.size, fresh.chunk.overlap);
                state.chunk_config = fresh.chunk;
                self.store
                    .lock()
                    .forget_fingerprints()
                    .map_err(store_error)?;
            }
            state.ocr_config = fresh.ocr.clone();
            state.rerank_config = fresh.rerank.clone();
            state.candidate_pool = fresh.rerank.candidates.max(20);
            state.stamp = stamp;
        }
        Ok(RuntimeSnapshot {
            embedder: state.embedder.clone(),
            vision: state.vision.clone(),
            vectors: state.vectors.clone(),
            splitter: state.splitter.clone(),
            candidate_pool: state.candidate_pool,
            ocr: state.ocr_config.clone(),
            rerank: state.rerank_config.clone(),
        })
    }

    async fn semantic_ids(&self, query: &str, limit: usize) -> Vec<i64> {
        let runtime = match self.runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                *self.last_embed_error.lock() = Some(error.to_string());
                return Vec::new();
            }
        };
        let Some(embedder) = &runtime.embedder else {
            return Vec::new();
        };
        let result = async {
            let query = embedder.embed_query(query).await?;
            runtime.vectors.search(&query, limit).await
        }
        .await;
        match result {
            Ok(ids) => ids,
            Err(error) => {
                *self.last_embed_error.lock() = Some(error.to_string());
                tracing::debug!(%error, "native semantic search unavailable; using keyword results");
                Vec::new()
            }
        }
    }

    async fn embed_pending(&self) -> Result<usize, RagError> {
        let runtime = self.runtime()?;
        let Some(embedder) = &runtime.embedder else {
            return Err(RagError::Unavailable(
                "chưa chọn mô hình nhúng. Tài liệu vẫn tìm được bằng từ khoá.".into(),
            ));
        };
        let rows = self.store.lock().all_chunks().map_err(store_error)?;
        if rows.is_empty() {
            return Ok(0);
        }
        let probe_text = embedding_text_for(&rows[0].section, &rows[0].body);
        let probe = embedder.embed_documents(&[probe_text]).await?;
        let dim = probe
            .first()
            .map(Vec::len)
            .filter(|dim| *dim > 0)
            .ok_or_else(|| {
                RagError::Service(format!("model `{}` trả về vector rỗng", embedder.model()))
            })?;
        let rebuilt = runtime
            .vectors
            .ensure(dim, embedder.model(), EMBED_INPUT_VERSION)
            .await?;
        let ids: Vec<_> = rows.iter().map(|row| row.id).collect();
        let existing: HashSet<_> = if rebuilt {
            HashSet::new()
        } else {
            runtime
                .vectors
                .existing_ids(&ids)
                .await?
                .into_iter()
                .collect()
        };
        let pending: Vec<_> = rows
            .into_iter()
            .filter(|row| !existing.contains(&row.id))
            .collect();
        let mut total = 0;
        for batch in pending.chunks(MAX_BATCH) {
            let texts: Vec<_> = batch
                .iter()
                .map(|row| embedding_text_for(&row.section, &row.body))
                .collect();
            let vectors = embedder.embed_documents(&texts).await?;
            let ids: Vec<_> = batch.iter().map(|row| row.id).collect();
            let payloads: Vec<_> = batch
                .iter()
                .map(|row| {
                    json!({
                        "document_id": row.document_id,
                        "ordinal": row.ordinal,
                        "page": row.page,
                    })
                })
                .collect();
            runtime
                .vectors
                .upsert(
                    &ids,
                    &vectors,
                    &payloads,
                    embedder.model(),
                    EMBED_INPUT_VERSION,
                )
                .await?;
            total += batch.len();
        }
        *self.last_embed_error.lock() = None;
        Ok(total)
    }

    fn rows_to_hits(
        rows: HashMap<i64, ChunkRow>,
        ranked: impl IntoIterator<Item = (i64, f32, MatchedBy)>,
    ) -> Vec<Hit> {
        ranked
            .into_iter()
            .filter_map(|(id, score, matched_by)| {
                rows.get(&id).map(|row| hit(row, score, matched_by))
            })
            .collect()
    }

    fn run(&self, source: Source) -> BoxStream<'_, IngestEvent> {
        let root = self.root.display().to_string();
        let started = event(IngestStage::Reading, root.clone(), None, 0, 0);
        let tail = async move { self.process(source).await };
        stream::once(async move { started })
            .chain(stream::once(tail).flat_map(stream::iter))
            .boxed()
    }

    async fn process(&self, source: Source) -> Vec<IngestEvent> {
        let result = self.process_inner(source).await;
        // Clear even when SQLite/config/extraction aborts the whole pass; otherwise the
        // UI would keep reporting a scan that can never make progress.
        *self.scanning.lock() = None;
        match result {
            Ok(report) => report.events(&self.root),
            Err(error) => vec![
                event(
                    IngestStage::Failed,
                    self.root.display().to_string(),
                    Some(error.to_string()),
                    0,
                    0,
                ),
                event(
                    IngestStage::Finished,
                    self.root.display().to_string(),
                    None,
                    0,
                    0,
                ),
            ],
        }
    }

    async fn process_inner(&self, source: Source) -> Result<RunReport, RagError> {
        if matches!(source, Source::Reprocess) {
            self.store
                .lock()
                .forget_fingerprints()
                .map_err(store_error)?;
        }
        let (paths, over_limit, scan_mode) = match source {
            Source::Sync | Source::Reprocess => {
                if !self.root.is_dir() {
                    return Err(RagError::Service(format!(
                        "thư mục dự án `{}` không tồn tại",
                        self.root.display()
                    )));
                }
                let root = self.root.clone();
                let (paths, over) =
                    tokio::task::spawn_blocking(move || extract::scan(&root, MAX_FILES))
                        .await
                        .map_err(|error| {
                            RagError::Service(format!("luồng quét bị dừng: {error}"))
                        })?;
                (paths, over, true)
            }
            Source::Paths(paths) => (paths, 0, false),
        };
        let total = paths.len() as u32;
        *self.scanning.lock() = Some(Scanning { done: 0, total });
        let known = self.store.lock().known_files().map_err(store_error)?;
        let excluded = self.store.lock().excluded().map_err(store_error)?;
        let mut report = RunReport {
            scanned: total,
            over_limit: over_limit as u32,
            ..RunReport::default()
        };

        for (index, path) in paths.into_iter().enumerate() {
            let shown = path.display().to_string();
            if scan_mode && excluded.contains(&shown) {
                report.excluded += 1;
                continue;
            }
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    report
                        .failures
                        .push((shown, format!("không đọc được thuộc tính: {error}")));
                    continue;
                }
            };
            let fingerprint = (modified_seconds(&metadata), metadata.len() as i64);
            if scan_mode && known.get(&shown) == Some(&fingerprint) {
                report.unchanged += 1;
                *self.scanning.lock() = Some(Scanning {
                    done: index as u32 + 1,
                    total,
                });
                continue;
            }

            let outcome = match reader_for(&path) {
                Some(ReaderKind::Native(format)) => self.ingest_native(&path, format).await,
                Some(ReaderKind::Pdf) => self.ingest_pdf(&path).await,
                Some(ReaderKind::Image) => self.ingest_image(&path).await,
                Some(ReaderKind::Unsupported) => Err(RagError::Extraction(format!(
                    "`{shown}` cần bộ đọc chưa có trong bản Rust"
                ))),
                None => Err(RagError::Service(format!(
                    "không hỗ trợ định dạng của `{shown}`"
                ))),
            };
            match outcome {
                Ok(_) => report.ingested += 1,
                Err(error) => {
                    let reason = error.to_string();
                    if scan_mode && error.is_extraction() {
                        self.store
                            .lock()
                            .put_failure(&shown, fingerprint.0, fingerprint.1, &reason)
                            .map_err(store_error)?;
                    }
                    report.failures.push((shown, reason));
                }
            }
            *self.scanning.lock() = Some(Scanning {
                done: index as u32 + 1,
                total,
            });
        }

        match self.embed_pending().await {
            Ok(count) => report.embedded = count as u32,
            Err(error) => {
                report.embed_error = Some(error.to_string());
                *self.last_embed_error.lock() = report.embed_error.clone();
            }
        }
        if scan_mode {
            let store = self.store.lock();
            store
                .set_meta(
                    pai_rag_core::store::META_SCAN_FILES,
                    &report.scanned.to_string(),
                )
                .map_err(store_error)?;
            store
                .set_meta(
                    pai_rag_core::store::META_SCAN_SKIPPED,
                    &report.over_limit.to_string(),
                )
                .map_err(store_error)?;
            store
                .set_meta(pai_rag_core::store::META_SCAN_AT, &now_millis().to_string())
                .map_err(store_error)?;
        }
        Ok(report)
    }

    async fn ingest_native(&self, path: &Path, format: Format) -> Result<String, RagError> {
        let owned = path.to_owned();
        let extracted = tokio::task::spawn_blocking(move || extract::extract(&owned, format))
            .await
            .map_err(|error| RagError::Service(format!("luồng đọc tệp bị dừng: {error}")))??;
        self.store_extracted(path, extracted).await
    }

    async fn ingest_pdf(&self, path: &Path) -> Result<String, RagError> {
        let runtime = self.runtime()?;
        let owned = path.to_owned();
        let mut pages = tokio::task::spawn_blocking(move || extract::pdf_text_pages(&owned))
            .await
            .map_err(|error| RagError::Service(format!("luồng đọc PDF bị dừng: {error}")))??;
        let average_chars = extract::average_chars(&pages);
        let has_sparse_page = pages
            .iter()
            .any(|page| page.trim().chars().count() < runtime.ocr.min_chars_per_page);
        if !has_sparse_page {
            return self
                .store_extracted(path, extract::pdf_from_pages(path, pages, Vec::new()))
                .await;
        }
        if !runtime.ocr.enabled {
            // A mixed PDF is still useful without OCR: keep its native text instead of rejecting the whole
            // document because a cover or illustration page is blank. Fully scanned PDFs remain actionable
            // failures, so the UI can tell the user to enable OCR.
            if average_chars >= runtime.ocr.min_chars_per_page {
                return self
                    .store_extracted(path, extract::pdf_from_pages(path, pages, Vec::new()))
                    .await;
            }
            return Err(RagError::Extraction(format!(
                "PDF `{}` không có đủ lớp chữ và OCR đang tắt",
                path.display()
            )));
        }
        let Some(vision) = runtime.vision else {
            if average_chars >= runtime.ocr.min_chars_per_page {
                return self
                    .store_extracted(path, extract::pdf_from_pages(path, pages, Vec::new()))
                    .await;
            }
            return Err(RagError::Extraction(
                "đây là PDF quét; hãy chọn mô hình vision trong Cài đặt rồi xử lý lại".into(),
            ));
        };
        let owned = path.to_owned();
        let scale = runtime.ocr.scale;
        let limit = runtime.ocr.max_pages.max(1);
        let images =
            tokio::task::spawn_blocking(move || extract::render_pdf_pages(&owned, scale, limit))
                .await
                .map_err(|error| RagError::Service(format!("luồng dựng PDF bị dừng: {error}")))??;
        let mut ocr_pages = Vec::new();
        for (index, image) in images.iter().enumerate() {
            if pages
                .get(index)
                .is_some_and(|page| page.trim().chars().count() >= runtime.ocr.min_chars_per_page)
            {
                continue;
            }
            match vision.ocr(image, "image/png").await {
                Ok(text) if !text.is_empty() => {
                    if let Some(page) = pages.get_mut(index) {
                        *page = text;
                        ocr_pages.push(index as u32 + 1);
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, page = index + 1, path = %path.display(), "OCR page failed")
                }
            }
        }
        if !pages.iter().any(|page| !page.trim().is_empty()) {
            return Err(RagError::Extraction(format!(
                "model vision `{}` đã chạy nhưng không đọc được chữ trong `{}`",
                vision.model(),
                path.display()
            )));
        }
        let extracted = extract::pdf_from_pages(path, pages, ocr_pages);
        self.store_extracted(path, extracted).await
    }

    async fn ingest_image(&self, path: &Path) -> Result<String, RagError> {
        let runtime = self.runtime()?;
        if !runtime.ocr.enabled {
            return Err(RagError::Extraction(format!(
                "`{}` là ảnh và OCR đang tắt",
                path.display()
            )));
        }
        let vision = runtime.vision.ok_or_else(|| {
            RagError::Extraction(
                "đây là ảnh; hãy chọn mô hình vision trong Cài đặt rồi nạp lại".into(),
            )
        })?;
        let owned = path.to_owned();
        let (bytes, mime) = tokio::task::spawn_blocking(move || extract::image_bytes(&owned))
            .await
            .map_err(|error| RagError::Service(format!("luồng đọc ảnh bị dừng: {error}")))??;
        let text = vision.ocr(&bytes, mime).await?;
        if text.is_empty() {
            return Err(RagError::Extraction(format!(
                "model vision `{}` không đọc được chữ trong `{}`",
                vision.model(),
                path.display()
            )));
        }
        let extracted = extract::Extracted {
            text,
            format: Format::Image,
            title: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            pages: 1,
            ocr_pages: vec![1],
        };
        self.store_extracted(path, extracted).await
    }

    async fn store_extracted(
        &self,
        path: &Path,
        extracted: extract::Extracted,
    ) -> Result<String, RagError> {
        let runtime = self.runtime()?;
        let chunks = runtime.splitter.split(&extracted.text);
        if chunks.is_empty() {
            return Err(RagError::Extraction(format!(
                "đọc được `{}` nhưng không cắt ra đoạn nào",
                path.display()
            )));
        }
        let metadata = std::fs::metadata(path)
            .map_err(|error| RagError::Service(format!("không đọc được thuộc tính: {error}")))?;
        let id = document_id(&self.root, path);
        if runtime.embedder.is_some()
            && let Err(error) = runtime.vectors.remove_document(&id).await
        {
            tracing::debug!(%error, document = %id, "could not clear stale vectors");
        }
        self.store
            .lock()
            .put_document(&DocumentInput {
                id: &id,
                path: &path.display().to_string(),
                title: &extracted.title,
                format: extracted.format.as_str(),
                bytes: metadata.len() as i64,
                mtime: modified_seconds(&metadata),
                pages: i64::from(extracted.pages),
                ocr_pages: &extracted.ocr_pages,
                chunks: &chunks,
            })
            .map_err(store_error)?;
        Ok(id)
    }
}

#[async_trait]
impl DocLibrary for NativeLibrary {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError> {
        let limit = limit.clamp(1, 30);
        let runtime = self.runtime()?;
        let keyword_only = prefers_keyword(query) || runtime.embedder.is_none();
        let pool = runtime.candidate_pool.max(limit * 4);
        let keyword = self
            .store
            .lock()
            .search_keyword(query, if keyword_only { limit } else { pool })
            .map_err(store_error)?;
        if keyword_only {
            let rows = rows_by_id(&self.store.lock(), &keyword)?;
            return Ok(Self::rows_to_hits(
                rows,
                keyword
                    .into_iter()
                    .enumerate()
                    .map(|(rank, id)| (id, 1.0 / (rank + 1) as f32, MatchedBy::Keyword)),
            ));
        }
        let semantic = self.semantic_ids(query, pool).await;
        let ranked = fuse(&keyword, &semantic, pool);
        let ids: Vec<_> = ranked.iter().map(|row| row.chunk_id).collect();
        let rows = rows_by_id(&self.store.lock(), &ids)?;
        if runtime.rerank.enabled && !runtime.rerank.backend.eq_ignore_ascii_case("http") {
            tracing::warn!(
                backend = %runtime.rerank.backend,
                "unsupported native rerank backend; keeping RRF order"
            );
        }
        if runtime.rerank.enabled && runtime.rerank.backend.eq_ignore_ascii_case("http") {
            let passages: Vec<_> = ranked
                .iter()
                .filter_map(|row| rows.get(&row.chunk_id).map(|chunk| chunk.body.as_str()))
                .collect();
            match rerank::http(&runtime.rerank, query, &passages, limit).await {
                Ok(scored) => {
                    let ordered: Vec<_> = ranked
                        .iter()
                        .filter(|row| rows.contains_key(&row.chunk_id))
                        .collect();
                    return Ok(Self::rows_to_hits(
                        rows,
                        scored.into_iter().filter_map(|item| {
                            let ranked = ordered.get(item.index)?;
                            let matched = match ranked.matched_by {
                                CoreMatchedBy::Keyword => MatchedBy::Keyword,
                                CoreMatchedBy::Semantic => MatchedBy::Semantic,
                                CoreMatchedBy::Both => MatchedBy::Both,
                            };
                            Some((ranked.chunk_id, item.score, matched))
                        }),
                    ));
                }
                Err(error) => tracing::warn!(
                    %error,
                    "HTTP reranker unavailable; keeping native RRF order"
                ),
            }
        }
        Ok(Self::rows_to_hits(
            rows,
            ranked.into_iter().take(limit).map(|row| {
                let matched = match row.matched_by {
                    CoreMatchedBy::Keyword => MatchedBy::Keyword,
                    CoreMatchedBy::Semantic => MatchedBy::Semantic,
                    CoreMatchedBy::Both => MatchedBy::Both,
                };
                (row.chunk_id, row.score as f32, matched)
            }),
        ))
    }

    async fn documents(&self) -> Result<Vec<Document>, RagError> {
        let rows = self.store.lock().documents().map_err(store_error)?;
        Ok(rows.into_iter().map(document).collect())
    }

    async fn chunks(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Hit>, RagError> {
        let rows = self
            .store
            .lock()
            .chunks_of(document_id, offset, limit.clamp(1, 30))
            .map_err(store_error)?;
        Ok(rows
            .iter()
            .map(|row| hit(row, 0.0, MatchedBy::Keyword))
            .collect())
    }

    async fn stats(&self) -> Result<Stats, RagError> {
        let runtime = self.runtime()?;
        let (documents, chunks, failures, excluded, files_seen, files_skipped, scanned_at) = {
            let store = self.store.lock();
            let (documents, chunks) = store.counts().map_err(store_error)?;
            let failures = store.failures().map_err(store_error)?.len();
            let excluded = store.excluded().map_err(store_error)?.len();
            let number = |key| -> u32 {
                store
                    .meta(key)
                    .ok()
                    .flatten()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0)
            };
            (
                documents as u32,
                chunks as u32,
                failures as u32,
                excluded as u32,
                number(pai_rag_core::store::META_SCAN_FILES),
                number(pai_rag_core::store::META_SCAN_SKIPPED),
                store
                    .meta(pai_rag_core::store::META_SCAN_AT)
                    .ok()
                    .flatten()
                    .and_then(|value| value.parse().ok()),
            )
        };
        let (vectors, reachable) = if runtime.embedder.is_none() {
            // Keyword-only mode must not pay a network timeout just to rediscover that
            // semantic search is disabled by configuration.
            (0, false)
        } else {
            match runtime.vectors.count().await {
                Ok(count) => (count as u32, true),
                Err(error) => {
                    *self.last_embed_error.lock() = Some(error.to_string());
                    (0, false)
                }
            }
        };
        let semantic_ready = reachable && chunks > 0 && vectors >= chunks;
        let reason = if semantic_ready {
            None
        } else if runtime.embedder.is_none() {
            Some("Chưa chọn mô hình nhúng; tìm kiếm đang dùng từ khoá.".into())
        } else if let Some(error) = self.last_embed_error.lock().clone() {
            Some(error)
        } else if !reachable {
            Some("Qdrant không trả lời; tìm kiếm đang dùng từ khoá.".into())
        } else if chunks == 0 {
            Some("Thư viện chưa có đoạn nào để nhúng.".into())
        } else {
            Some(format!(
                "Còn {} đoạn chưa được nhúng.",
                chunks.saturating_sub(vectors)
            ))
        };
        Ok(Stats {
            documents,
            chunks,
            embedded_chunks: vectors,
            embedder: runtime
                .embedder
                .as_ref()
                .map(|client| client.model().to_owned()),
            semantic_ready,
            reason,
            root: self.root.clone(),
            files_seen,
            files_skipped,
            unreadable: failures,
            excluded,
            scanned_at,
            scanning: *self.scanning.lock(),
        })
    }

    fn sync(&self) -> BoxStream<'_, IngestEvent> {
        self.run(Source::Sync)
    }

    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        self.run(Source::Paths(paths))
    }

    fn reprocess(&self) -> BoxStream<'_, IngestEvent> {
        self.run(Source::Reprocess)
    }

    async fn remove(&self, id: &str) -> Result<(), RagError> {
        let found = self.store.lock().document(id).map_err(store_error)?;
        let Some(found) = found else {
            return Err(RagError::NotFound(id.to_owned()));
        };
        {
            let mut store = self.store.lock();
            store
                .exclude(&found.path, now_millis())
                .map_err(store_error)?;
            store.remove_document(id).map_err(store_error)?;
        }
        let runtime = self.runtime()?;
        if let Err(error) = runtime.vectors.remove_document(id).await {
            tracing::debug!(%error, document = id, "could not remove Qdrant points");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum Source {
    Sync,
    Paths(Vec<PathBuf>),
    Reprocess,
}

#[derive(Default)]
struct RunReport {
    scanned: u32,
    ingested: u32,
    unchanged: u32,
    over_limit: u32,
    excluded: u32,
    embedded: u32,
    failures: Vec<(String, String)>,
    embed_error: Option<String>,
}

impl RunReport {
    fn events(self, root: &Path) -> Vec<IngestEvent> {
        let mut events = Vec::new();
        for (path, reason) in self.failures {
            events.push(event(
                IngestStage::Failed,
                path,
                Some(reason),
                self.ingested,
                self.scanned,
            ));
        }
        if let Some(error) = self.embed_error {
            events.push(event(
                IngestStage::Embedding,
                root.display().to_string(),
                Some(error),
                self.ingested,
                self.scanned,
            ));
        }
        events.push(event(
            IngestStage::Finished,
            root.display().to_string(),
            None,
            self.ingested,
            self.scanned,
        ));
        events
    }
}

fn reconcile(store: &mut Store, config: &NativeConfig) -> Result<(), RagError> {
    let seen = store.identity().map_err(store_error)?;
    let stale_extract = seen.extract.as_deref() != Some(&EXTRACT_VERSION.to_string());
    let stale_input = seen.embed_input.as_deref() != Some(&EMBED_INPUT_VERSION.to_string());
    if stale_extract || stale_input {
        store.forget_fingerprints().map_err(store_error)?;
    }
    store
        .set_identity(
            &config.embedding.model,
            config.embedding.dim,
            EMBED_INPUT_VERSION,
            EXTRACT_VERSION,
        )
        .map_err(store_error)
}

fn rows_by_id(store: &Store, ids: &[i64]) -> Result<HashMap<i64, ChunkRow>, RagError> {
    Ok(store
        .chunks_by_id(ids)
        .map_err(store_error)?
        .into_iter()
        .map(|row| (row.id, row))
        .collect())
}

fn hit(row: &ChunkRow, score: f32, matched_by: MatchedBy) -> Hit {
    Hit {
        document_id: row.document_id.clone(),
        title: row.title.clone(),
        path: PathBuf::from(&row.path),
        ordinal: row.ordinal as u32,
        heading: (!row.section.trim().is_empty()).then(|| row.section.clone()),
        text: row.body.clone(),
        score,
        matched_by,
        page: row.page as u32,
    }
}

fn document(row: pai_rag_core::DocumentRow) -> Document {
    let chunks = row.chunks as u32;
    Document {
        id: row.id,
        path: PathBuf::from(&row.path),
        origin: row.path,
        title: row.title,
        format: Format::parse(&row.format),
        bytes: row.bytes as u64,
        chunks,
        embedded: chunks > 0 && row.error.is_none(),
        added_at: row.added_at,
        error: row.error,
        pages: row.pages as u32,
        ocr_pages: row.ocr_pages,
    }
}

fn event(
    stage: IngestStage,
    path: String,
    error: Option<String>,
    done: u32,
    total: u32,
) -> IngestEvent {
    IngestEvent {
        path,
        stage,
        done,
        total,
        finished: stage == IngestStage::Finished,
        error,
        document: None,
    }
}

fn document_id(root: &Path, path: &Path) -> String {
    let identity = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let digest = Sha1::digest(identity.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn modified_seconds(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn config_stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn prefers_keyword(query: &str) -> bool {
    let quoted = query.contains('"') || (query.contains('“') && query.contains('”'));
    quoted
        || query.split_whitespace().any(|token| {
            token.len() >= 3
                && token.bytes().any(|b| b.is_ascii_digit())
                && token.chars().any(char::is_alphabetic)
        })
}

fn store_error(error: pai_rag_core::StoreError) -> RagError {
    RagError::Service(format!("kho tài liệu hỏng: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_ids_match_python_sha1_prefix() {
        assert_eq!(
            document_id(Path::new("/root"), Path::new("/root/docs/a.md")),
            "f00cbb922d5ef4be"
        );
    }

    #[test]
    fn identifiers_choose_keyword_search() {
        assert!(prefers_keyword("tìm HD-2026-0042"));
        assert!(prefers_keyword("\"exact phrase\""));
        assert!(!prefers_keyword("tài liệu nói về gì"));
    }
}
