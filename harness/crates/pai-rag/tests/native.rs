use std::{
    fs,
    sync::{Arc, Mutex},
    time::Instant,
};

use futures::StreamExt;
use pai_rag::{DocLibrary, IngestStage, NativeLibrary, purge_library};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

fn library() -> (tempfile::TempDir, NativeLibrary) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("documents");
    let data = temp.path().join("data");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("guide.md"),
        "# Cài đặt\n\nCấu hình dịch vụ native Rust rất nhanh.",
    )
    .unwrap();
    let config_path = temp.path().join("rag-config.json");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "data_dir": data,
            "projects": [{"id": "docs-test", "name": "Docs", "root": root}],
            "active_project": "docs-test",
            "embedding": {"kind": "ollama", "base_url": "", "api_key": "", "model": ""},
            "vectors": {"url": "http://127.0.0.1:1", "api_key": "", "collection_prefix": "test"},
            "chunk": {"size": 80, "overlap": 10}
        }))
        .unwrap(),
    )
    .unwrap();
    let library = NativeLibrary::open(config_path, "docs-test".into(), root).unwrap();
    (temp, library)
}

#[tokio::test]
async fn native_path_ingests_searches_reads_and_removes() {
    let (_temp, library) = library();

    let events: Vec<_> = library.sync().collect().await;
    assert_eq!(events.first().unwrap().stage, IngestStage::Reading);
    assert_eq!(events.last().unwrap().stage, IngestStage::Finished);
    assert!(events.iter().any(|event| {
        event.stage == IngestStage::Reading
            && event.path.ends_with("guide.md")
            && event.done == 0
            && event.total == 1
    }));
    assert!(events.iter().any(|event| {
        event.stage == IngestStage::Stored
            && event.path.ends_with("guide.md")
            && event.done == 1
            && event.total == 1
    }));
    assert!(
        events
            .iter()
            .any(|event| event.stage == IngestStage::Embedding)
    );

    let documents = library.documents().await.unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].title, "guide");

    let hits = library.search("cấu hình dịch vụ", 8).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.contains("native Rust"));
    let read = library.chunks(&documents[0].id, 0, 6).await.unwrap();
    assert_eq!(read[0].text, hits[0].text);

    library.remove(&documents[0].id).await.unwrap();
    assert!(library.documents().await.unwrap().is_empty());

    // The source file still exists, but exclusion keeps the next sync from undoing remove.
    library.sync().collect::<Vec<_>>().await;
    assert!(library.documents().await.unwrap().is_empty());
}

#[tokio::test]
async fn sync_fingerprints_unchanged_extraction_errors() {
    let (temp, library) = library();
    let root = temp.path().join("documents");
    fs::write(root.join("empty.txt"), "").unwrap();
    fs::write(root.join("scan.png"), b"not-a-real-image").unwrap();

    let first: Vec<_> = library.sync().collect().await;
    assert_eq!(
        first
            .iter()
            .filter(|event| event.stage == IngestStage::Failed)
            .count(),
        2
    );
    let second: Vec<_> = library.sync().collect().await;
    assert_eq!(
        second
            .iter()
            .filter(|event| event.stage == IngestStage::Failed)
            .count(),
        0,
        "unchanged extraction failures are fingerprinted and not retried"
    );
    assert_eq!(library.stats().await.unwrap().unreadable, 2);
}

#[tokio::test]
async fn native_embedding_and_qdrant_path_runs_end_to_end() {
    let mock = MockServices::start().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("documents");
    let data = temp.path().join("data");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("semantic.md"),
        "# Rust\n\nNative semantic retrieval.",
    )
    .unwrap();
    let config_path = temp.path().join("rag-config.json");
    let write_config = |model: &str| {
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "data_dir": data,
                "projects": [{"id": "docs-test", "root": root}],
                "active_project": "docs-test",
                "embedding": {
                    "kind": "ollama", "base_url": mock.url(), "api_key": "", "model": model
                },
                "rerank": {"enabled": false},
                "vectors": {"url": mock.url(), "api_key": "", "collection_prefix": "test"}
            }))
            .unwrap(),
        )
        .unwrap();
    };
    write_config("");
    let library =
        NativeLibrary::open(config_path.clone(), "docs-test".into(), root.clone()).unwrap();

    let first: Vec<_> = library.sync().collect().await;
    assert!(
        first
            .iter()
            .any(|event| event.stage == IngestStage::Embedding)
    );
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    write_config("qwen3-embedding:test");
    let events: Vec<_> = library.sync().collect().await;
    assert!(!events.iter().any(|event| event.error.is_some()));
    let hits = library.search("semantic retrieval", 8).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].matched_by.as_str(), "both");
    let stats = library.stats().await.unwrap();
    assert!(stats.semantic_ready);
    assert_eq!(stats.embedded_chunks, 1);

    let requests = mock.requests();
    assert!(
        requests
            .iter()
            .any(|line| line == "PUT /collections/test_docs-test")
    );
    assert!(
        requests
            .iter()
            .any(|line| line == "PUT /collections/test_docs-test/points")
    );
    assert!(
        requests
            .iter()
            .any(|line| line == "POST /collections/test_docs-test/points/query")
    );
    mock.stop();
}

#[tokio::test]
async fn native_purge_removes_only_index_data() {
    let temp = tempfile::tempdir().unwrap();
    let user_root = temp.path().join("user-documents");
    let data = temp.path().join("app-data");
    let index = data.join("docs-test");
    fs::create_dir_all(&user_root).unwrap();
    fs::create_dir_all(&index).unwrap();
    fs::write(user_root.join("keep.md"), "do not delete").unwrap();
    fs::write(index.join("rag.sqlite"), "index").unwrap();
    let config = temp.path().join("rag-config.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({
            "version": 1,
            "data_dir": data,
            "vectors": {"url": "http://127.0.0.1:1", "collection_prefix": "test"}
        }))
        .unwrap(),
    )
    .unwrap();

    purge_library(&config, "docs-test").await.unwrap();
    assert!(!index.exists());
    assert_eq!(
        fs::read_to_string(user_root.join("keep.md")).unwrap(),
        "do not delete"
    );
    assert!(purge_library(&config, "..").await.is_err());
}

#[tokio::test]
#[ignore = "manual native cold/incremental performance baseline"]
async fn native_sync_scale() {
    const FILES: usize = 1_000;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("documents");
    let data = temp.path().join("data");
    fs::create_dir_all(&root).unwrap();
    for index in 0..FILES {
        fs::write(
            root.join(format!("doc-{index:04}.md")),
            format!("# Tài liệu {index}\n\nNội dung native Rust số {index}."),
        )
        .unwrap();
    }
    let config_path = temp.path().join("rag-config.json");
    fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "version": 1,
            "data_dir": data,
            "projects": [{"id": "scale", "root": root}],
            "active_project": "scale",
            "embedding": {"model": ""},
            "rerank": {"enabled": false},
            "vectors": {"url": "http://127.0.0.1:1"}
        }))
        .unwrap(),
    )
    .unwrap();
    let library = NativeLibrary::open(config_path, "scale".into(), root).unwrap();

    let started = Instant::now();
    library.sync().collect::<Vec<_>>().await;
    let cold = started.elapsed();
    let started = Instant::now();
    library.sync().collect::<Vec<_>>().await;
    let warm = started.elapsed();
    assert_eq!(library.documents().await.unwrap().len(), FILES);
    eprintln!("native sync: {FILES} files cold={cold:?}, unchanged={warm:?}");
}

struct MockServices {
    address: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl MockServices {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            let mut collection_exists = false;
            let mut point_count = 0usize;
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8_192];
                let (header_end, content_length) = loop {
                    let Ok(read) = socket.read(&mut chunk).await else {
                        break (0, 0);
                    };
                    if read == 0 {
                        break (0, 0);
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                        let end = end + 4;
                        let headers = String::from_utf8_lossy(&buffer[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(str::trim)
                                    .and_then(|value| value.parse().ok())
                            })
                            .unwrap_or(0);
                        if buffer.len() >= end + length {
                            break (end, length);
                        }
                    }
                };
                if header_end == 0 {
                    continue;
                }
                let headers = String::from_utf8_lossy(&buffer[..header_end]);
                let first = headers.lines().next().unwrap();
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap();
                let target = parts.next().unwrap();
                let path = target.split('?').next().unwrap();
                recorded.lock().unwrap().push(format!("{method} {path}"));
                let body: serde_json::Value = if content_length == 0 {
                    json!(null)
                } else {
                    serde_json::from_slice(&buffer[header_end..header_end + content_length])
                        .unwrap()
                };

                let (status, payload) = match (method, path) {
                    ("POST", "/api/embed") => {
                        let count = body
                            .get("input")
                            .and_then(|value| value.as_array())
                            .unwrap()
                            .len();
                        (200, json!({"embeddings": vec![vec![1.0, 0.0]; count]}))
                    }
                    ("GET", "/collections/test_docs-test") if !collection_exists => {
                        (404, json!({"status": "not found"}))
                    }
                    ("GET", "/collections/test_docs-test") => (
                        200,
                        json!({
                            "status": "ok", "result": {"config": {"params": {"vectors": {"size": 2}}}}
                        }),
                    ),
                    ("PUT", "/collections/test_docs-test") => {
                        assert_eq!(
                            body.pointer("/vectors/size")
                                .and_then(|value| value.as_u64()),
                            Some(2)
                        );
                        collection_exists = true;
                        (200, json!({"status": "ok", "result": true}))
                    }
                    ("PUT", "/collections/test_docs-test/index") => {
                        assert_eq!(
                            body.get("field_name").and_then(|value| value.as_str()),
                            Some("document_id")
                        );
                        (
                            200,
                            json!({"status": "ok", "result": {"status": "acknowledged"}}),
                        )
                    }
                    ("PUT", "/collections/test_docs-test/points") => {
                        point_count = body
                            .get("points")
                            .and_then(|value| value.as_array())
                            .unwrap()
                            .len();
                        assert!(body.pointer("/points/0/payload/_embed_model").is_some());
                        (
                            200,
                            json!({"status": "ok", "result": {"status": "completed"}}),
                        )
                    }
                    ("POST", "/collections/test_docs-test/points/query") => {
                        assert!(
                            body.get("query")
                                .and_then(|value| value.as_array())
                                .is_some()
                        );
                        (
                            200,
                            json!({"status": "ok", "result": {"points": [{"id": 1, "score": 1.0}]}}),
                        )
                    }
                    ("POST", "/collections/test_docs-test/points/count") => (
                        200,
                        json!({"status": "ok", "result": {"count": point_count}}),
                    ),
                    _ => panic!("unexpected mock request: {method} {path} {body}"),
                };
                let reason = if status == 200 { "OK" } else { "Not Found" };
                let payload = serde_json::to_vec(&payload).unwrap();
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.write_all(&payload).await.unwrap();
            }
        });
        Self {
            address,
            requests,
            task,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn stop(self) {
        self.task.abort();
    }
}
