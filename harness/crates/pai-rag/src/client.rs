//! [`DocLibrary`] talking to `pai-rag-service` over MCP.
//! JSON from another process is read defensively: every field has a default that
//! understates rather than invents, and the three ingest streams wrap a single MCP call.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::{Map, Value, json};

use crate::error::RagError;
use crate::format::Format;
use crate::library::{DocLibrary, Document, Hit, IngestEvent, IngestStage, Stats};
use crate::search::MatchedBy;
use crate::sidecar::Sidecar;

pub struct RagClient {
    sidecar: Arc<Sidecar>,
    /// Project folder, kept here so [`Stats::root`] and progress ticks have it before the service ever answers.
    root: PathBuf,
}

impl RagClient {
    pub fn new(sidecar: Arc<Sidecar>, root: PathBuf) -> RagClient {
        RagClient { sidecar, root }
    }

    pub async fn shutdown(&self) {
        self.sidecar.shutdown().await;
    }

    /// One ingest run, wrapped as a stream of progress ticks.
    fn ingest_stream<'a>(
        &'a self,
        tool: &'a str,
        args: Map<String, Value>,
    ) -> BoxStream<'a, IngestEvent> {
        let root = self.root.display().to_string();
        let started = IngestEvent {
            path: root.clone(),
            stage: IngestStage::Reading,
            done: 0,
            total: 0,
            finished: false,
            error: None,
            document: None,
        };

        let tail = async move {
            let outcome = self.sidecar.call(tool, args).await;
            events_from_report(&root, outcome)
        };

        stream::once(async move { started })
            .chain(stream::once(tail).flat_map(stream::iter))
            .boxed()
    }
}

#[async_trait]
impl DocLibrary for RagClient {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>, RagError> {
        let mut args = Map::new();
        args.insert("query".into(), json!(query));
        args.insert("limit".into(), json!(limit));
        let value = self.sidecar.call("docs.search", args).await?;
        Ok(read_hits(&value))
    }

    async fn documents(&self) -> Result<Vec<Document>, RagError> {
        let value = self.sidecar.call("docs.list", Map::new()).await?;
        Ok(array(&value, "documents")
            .iter()
            .filter_map(read_document)
            .collect())
    }

    async fn chunks(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Hit>, RagError> {
        let mut args = Map::new();
        args.insert("document_id".into(), json!(document_id));
        args.insert("offset".into(), json!(offset));
        args.insert("limit".into(), json!(limit));
        let value = self.sidecar.call("docs.read", args).await?;
        Ok(read_hits(&value))
    }

    async fn stats(&self) -> Result<Stats, RagError> {
        let value = self.sidecar.call("docs.stats", Map::new()).await?;
        Ok(read_stats(&value, &self.root, self.sidecar.last_error()))
    }

    fn sync(&self) -> BoxStream<'_, IngestEvent> {
        self.ingest_stream("docs.sync", Map::new())
    }

    fn ingest(&self, paths: Vec<PathBuf>) -> BoxStream<'_, IngestEvent> {
        let mut args = Map::new();
        args.insert(
            "paths".into(),
            json!(paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()),
        );
        self.ingest_stream("docs.ingest", args)
    }

    fn reprocess(&self) -> BoxStream<'_, IngestEvent> {
        self.ingest_stream("docs.reprocess", Map::new())
    }

    async fn remove(&self, id: &str) -> Result<(), RagError> {
        let mut args = Map::new();
        args.insert("document_id".into(), json!(id));
        let value = self.sidecar.call("docs.remove", args).await?;
        if value.get("removed").and_then(Value::as_bool) == Some(false) {
            return Err(RagError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

/* ── read JSON ───────────────────────────────────────────────────────────────────── */

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn read_hits(value: &Value) -> Vec<Hit> {
    array(value, "hits").iter().filter_map(read_hit).collect()
}

fn read_hit(value: &Value) -> Option<Hit> {
    let document_id = value.get("documentId").and_then(Value::as_str)?;
    if document_id.is_empty() {
        return None;
    }
    let heading = value
        .get("section")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|found| !found.is_empty())
        .map(str::to_string);
    Some(Hit {
        document_id: document_id.to_string(),
        title: text(value, "title"),
        path: PathBuf::from(text(value, "path")),
        ordinal: number(value, "ordinal") as u32,
        heading,
        text: text(value, "text"),
        score: value.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        matched_by: MatchedBy::parse(&text(value, "matchedBy")),
        page: number(value, "page") as u32,
    })
}

fn read_document(value: &Value) -> Option<Document> {
    let id = value.get("documentId").and_then(Value::as_str)?;
    let path = text(value, "path");
    let chunks = number(value, "chunks") as u32;
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(Document {
        id: id.to_string(),
        path: PathBuf::from(&path),
        // The service has no notion of "where a file came from": the project folder *is* the library.
        origin: path,
        title: text(value, "title"),
        format: Format::parse(&text(value, "format")),
        bytes: number(value, "bytes"),
        chunks,
        // The service reports embedding library-wide, not per document; `stats()` is where the backlog is named.
        embedded: chunks > 0 && error.is_none(),
        added_at: value.get("addedAt").and_then(Value::as_i64).unwrap_or(0),
        error,
        pages: number(value, "pages") as u32,
        ocr_pages: array(value, "ocrPages")
            .iter()
            .filter_map(Value::as_u64)
            .map(|page| page as u32)
            .collect(),
    })
}

fn read_stats(value: &Value, root: &PathBuf, sidecar_error: Option<String>) -> Stats {
    let chunks = number(value, "chunks") as u32;
    let vectors = number(value, "vectors") as u32;
    let reachable = value
        .get("qdrant_reachable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let embedder = value
        .get("embedder")
        .and_then(Value::as_str)
        .filter(|found| !found.is_empty())
        .map(str::to_string);

    // Ready means *every* chunk has a vector; a half-embedded library still answers, but the other half is invisible to semantic search.
    let semantic_ready = reachable && chunks > 0 && vectors >= chunks;
    Stats {
        documents: number(value, "documents") as u32,
        chunks,
        embedded_chunks: vectors,
        embedder,
        semantic_ready,
        reason: reason_for(semantic_ready, reachable, chunks, vectors, sidecar_error),
        root: value
            .get("root")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone()),
        files_seen: number(value, "files_seen") as u32,
        files_skipped: number(value, "files_skipped") as u32,
        unreadable: array(value, "failures").len() as u32,
        excluded: number(value, "excluded") as u32,
        scanned_at: value.get("scanned_at").and_then(Value::as_i64),
        scanning: None,
    }
}

/// Why semantic search is unusable, in Vietnamese and naming the fix; ordered root cause outwards, since a dead service makes every other explanation wrong.
fn reason_for(
    ready: bool,
    reachable: bool,
    chunks: u32,
    vectors: u32,
    sidecar_error: Option<String>,
) -> Option<String> {
    if let Some(err) = sidecar_error {
        return Some(format!("Service tài liệu không trả lời: {err}"));
    }
    if ready {
        return None;
    }
    if !reachable {
        return Some(
            "Kho vector (Qdrant) chưa chạy nên chỉ có tìm theo từ khoá. Dựng nó bằng \
             `docker compose up -d` ở thư mục gốc dự án."
                .to_string(),
        );
    }
    if chunks == 0 {
        return Some(
            "Thư viện chưa có đoạn nào. Thả tệp vào thư mục dự án rồi bấm đồng bộ."
                .to_string(),
        );
    }
    Some(format!(
        "Đang nhúng: {vectors}/{chunks} đoạn đã có vector. Tìm theo từ khoá vẫn chạy \
         bình thường trong lúc chờ."
    ))
}

/// One ingest report -> the sequence of progress ticks for the UI.
fn events_from_report(root: &str, outcome: Result<Value, RagError>) -> Vec<IngestEvent> {
    let make = |stage: IngestStage, path: String, error: Option<String>, done: u32, total: u32| {
        IngestEvent {
            path,
            stage,
            done,
            total,
            finished: stage == IngestStage::Finished,
            error,
            document: None,
        }
    };

    let report = match outcome {
        Ok(value) => value,
        // Whole run failed: one `Failed` tick carrying the reason, then `Finished` so the progress bar stops spinning.
        Err(err) => {
            return vec![
                make(IngestStage::Failed, root.to_string(), Some(err.to_string()), 0, 0),
                make(IngestStage::Finished, root.to_string(), None, 0, 0),
            ];
        }
    };

    let scanned = number(&report, "scanned") as u32;
    let ingested = number(&report, "ingested") as u32;
    let mut events = Vec::new();

    for failure in array(&report, "failed") {
        events.push(make(
            IngestStage::Failed,
            text(failure, "path"),
            Some(text(failure, "reason")),
            ingested,
            scanned,
        ));
    }
    if let Some(error) = report
        .get("embed_error")
        .and_then(Value::as_str)
        .filter(|found| !found.is_empty())
    {
        // `Embedding`, not `Failed`: every file made it into the library, only the embedding server is behind.
        events.push(make(
            IngestStage::Embedding,
            root.to_string(),
            Some(error.to_string()),
            ingested,
            scanned,
        ));
    }
    events.push(make(IngestStage::Finished, root.to_string(), None, ingested, scanned));
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Missing fields must fall back to a value that understates the truth, never panic or invent a number.
    #[test]
    fn truong_thieu_khong_lam_sap_va_khong_bia() {
        let hit = read_hit(&json!({ "documentId": "abc" })).expect("có documentId là đủ");
        assert_eq!(hit.document_id, "abc");
        assert_eq!(hit.page, 0, "thiếu trang phải là 0 — giao diện khi ấy không vẽ trang");
        assert_eq!(hit.ordinal, 0);
        assert_eq!(hit.score, 0.0);
        assert!(hit.heading.is_none(), "mục rỗng không được thành một tiêu đề rỗng");
        assert_eq!(
            hit.matched_by,
            MatchedBy::Keyword,
            "nhãn lạ lùi về nhánh không cần bộ nhúng"
        );
    }

    /// The one exception: a chunk with no document id is dropped, because an uncitable quote is worse than none.
    #[test]
    fn doan_khong_co_ma_tai_lieu_bi_bo() {
        assert!(read_hit(&json!({ "title": "x", "text": "y" })).is_none());
        assert!(read_hit(&json!({ "documentId": "" })).is_none());

        let hits = read_hits(&json!({
            "hits": [
                { "documentId": "a", "title": "giữ" },
                { "title": "bỏ" },
            ]
        }));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "giữ");
    }

    #[test]
    fn doc_mot_ket_qua_day_du() {
        let hit = read_hit(&json!({
            "documentId": "d1",
            "title": "Quy trình",
            "path": "D:/tl/a.docx",
            "ordinal": 3,
            "section": "Phê duyệt",
            "page": 7,
            "text": "Trưởng bộ phận duyệt trong 24 giờ.",
            "score": 2.75,
            "matchedBy": "both",
        }))
        .unwrap();
        assert_eq!(hit.ordinal, 3);
        assert_eq!(hit.page, 7);
        assert_eq!(hit.heading.as_deref(), Some("Phê duyệt"));
        assert_eq!(hit.matched_by, MatchedBy::Both);
        assert!((hit.score - 2.75).abs() < 1e-6);
    }

    #[test]
    fn dinh_dang_la_khong_lam_mat_ca_danh_sach() {
        let doc = read_document(&json!({
            "documentId": "d1",
            "format": "dinh-dang-tuong-lai",
            "chunks": 4,
        }))
        .unwrap();
        assert_eq!(doc.format, Format::Text, "nhãn lạ lùi về text, không phải lỗi");
        assert!(doc.embedded, "có đoạn và không lỗi nghĩa là đã xong");

        let hong = read_document(&json!({
            "documentId": "d2",
            "chunks": 0,
            "error": "PDF cụt",
        }))
        .unwrap();
        assert!(!hong.embedded);
        assert_eq!(hong.error.as_deref(), Some("PDF cụt"));
    }

    /// `semantic_ready` holds only when *every* chunk has a vector; half-embedded results hide the other half.
    #[test]
    fn nhung_nua_chung_khong_phai_la_san_sang() {
        let root = PathBuf::from("D:/tl");
        let half = read_stats(
            &json!({ "chunks": 10, "vectors": 4, "qdrant_reachable": true }),
            &root,
            None,
        );
        assert!(!half.semantic_ready);
        let reason = half.reason.expect("phải nói ra vì sao");
        assert!(reason.contains("4/10"), "phải nói ra con số: {reason}");

        let full = read_stats(
            &json!({ "chunks": 10, "vectors": 10, "qdrant_reachable": true }),
            &root,
            None,
        );
        assert!(full.semantic_ready);
        assert!(full.reason.is_none(), "sẵn sàng thì không có gì để giải thích");
    }

    /// Explanations go root cause outwards: a dead service makes "still embedding" point at the wrong fix.
    #[test]
    fn ly_do_chi_dung_nguyen_nhan_goc() {
        let root = PathBuf::from("D:/tl");
        let chet = read_stats(
            &json!({ "chunks": 10, "vectors": 0, "qdrant_reachable": false }),
            &root,
            Some("không chạy được uv".to_string()),
        );
        let reason = chet.reason.unwrap();
        assert!(reason.contains("Service"), "{reason}");
        assert!(
            !reason.contains("Qdrant"),
            "đừng đổ lỗi cho Qdrant khi service đã chết"
        );

        let khong_qdrant = read_stats(
            &json!({ "chunks": 10, "vectors": 0, "qdrant_reachable": false }),
            &root,
            None,
        );
        assert!(khong_qdrant.reason.unwrap().contains("docker compose"));

        let trong = read_stats(&json!({ "chunks": 0, "qdrant_reachable": true }), &root, None);
        assert!(trong.reason.unwrap().contains("Thả tệp"));
    }

    /// A progress stream always ends with `Finished`; without it the UI progress bar spins forever.
    #[test]
    fn dong_tien_trinh_luon_dong_lai() {
        let hong = events_from_report("D:/tl", Err(RagError::Service("chết".into())));
        assert_eq!(hong.last().unwrap().stage, IngestStage::Finished);
        assert!(hong.last().unwrap().finished);
        assert_eq!(hong[0].stage, IngestStage::Failed);
        assert!(hong[0].error.as_deref().unwrap().contains("chết"));

        let xong = events_from_report(
            "D:/tl",
            Ok(json!({ "scanned": 5, "ingested": 5, "failed": [] })),
        );
        assert_eq!(xong.len(), 1);
        assert_eq!(xong[0].stage, IngestStage::Finished);
        assert_eq!(xong[0].done, 5);
        assert_eq!(xong[0].total, 5);
    }

    /// A lagging embedding server is `Embedding`, not `Failed` - the UI counts `Failed` as broken files.
    #[test]
    fn nhung_hong_khong_bi_dem_la_tep_hong() {
        let events = events_from_report(
            "D:/tl",
            Ok(json!({
                "scanned": 3,
                "ingested": 3,
                "failed": [],
                "embed_error": "không nối được Qdrant",
            })),
        );
        let stages: Vec<_> = events.iter().map(|item| item.stage).collect();
        assert_eq!(stages, vec![IngestStage::Embedding, IngestStage::Finished]);
    }

    #[test]
    fn tep_hong_duoc_ke_ten_kem_ly_do() {
        let events = events_from_report(
            "D:/tl",
            Ok(json!({
                "scanned": 2,
                "ingested": 1,
                "failed": [{ "path": "D:/tl/a.pdf", "reason": "PDF cụt" }],
            })),
        );
        assert_eq!(events[0].stage, IngestStage::Failed);
        assert_eq!(events[0].path, "D:/tl/a.pdf");
        assert_eq!(events[0].error.as_deref(), Some("PDF cụt"));
    }
}
