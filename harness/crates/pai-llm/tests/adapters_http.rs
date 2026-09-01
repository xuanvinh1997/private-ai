//! Hai adapter, chạy qua HTTP thật — nhưng với **một máy chủ dựng ngay trong bài test**.
//!
//! Không `wiremock`, không mạng: một `TcpListener` của tokio và vài chục dòng nói HTTP/1.1
//! là đủ, và nó cho ta thứ mà thư viện giả lập nào cũng giấu — quyền quyết định **byte
//! nào đi cùng gói nào**. Mọi phản hồi streaming ở đây được gửi theo `Transfer-Encoding:
//! chunked` với khung cố tình cắt lệch, nên đường ống thật phải chịu đúng cái cắt vụn mà
//! bài test đơn vị mô tả.

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use pai_llm::assembler::BlockAssembler;
use pai_llm::error::LlmErrorCode;
use pai_llm::message::{ChatRequest, Message, ToolSchema};
use pai_llm::model::ModelState;
use pai_llm::ollama::OllamaAdapter;
use pai_llm::openai::OpenAiAdapter;
use pai_llm::seam::LlmAdapter;
use pai_llm::stream::FinishReason;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// --- máy chủ giả ---------------------------------------------------------------------

#[derive(Clone)]
struct Route {
    status: u16,
    body: String,
    /// Gửi theo khung `chunked`, mỗi khung `slice` byte. `None` = một cục, Content-Length.
    slice: Option<usize>,
}

impl Route {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            slice: None,
        }
    }

    /// Phản hồi streaming, cắt thành từng khung `slice` byte — kể cả giữa ký tự UTF-8.
    fn streamed(body: impl Into<String>, slice: usize) -> Self {
        Self {
            status: 200,
            body: body.into(),
            slice: Some(slice),
        }
    }

    fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            slice: None,
        }
    }
}

/// Dựng một máy chủ HTTP tối giản, trả về base URL của nó.
async fn serve(routes: Vec<(&str, Route)>) -> String {
    let table: HashMap<String, Route> = routes
        .into_iter()
        .map(|(path, route)| (path.to_string(), route))
        .collect();
    let table = Arc::new(table);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("địa chỉ đã gán");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let table = table.clone();
            tokio::spawn(async move {
                let Some(path) = read_request(&mut socket).await else {
                    return;
                };
                let route = table
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| Route::status(404, "{\"error\":\"không có đường này\"}"));
                let _ = write_response(&mut socket, &route).await;
            });
        }
    });
    format!("http://{addr}")
}

/// Đọc trọn request, trả về đường dẫn. Thân được đọc hết để client không thấy reset.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 1024];
    let head_end = loop {
        let read = socket.read(&mut scratch).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&scratch[..read]);
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let read = socket.read(&mut scratch).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&scratch[..read]);
    }
    head.lines()
        .next()?
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
}

async fn write_response(socket: &mut tokio::net::TcpStream, route: &Route) -> std::io::Result<()> {
    let reason = if route.status == 200 { "OK" } else { "ERROR" };
    match route.slice {
        None => {
            let head = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                route.status,
                route.body.len()
            );
            socket.write_all(head.as_bytes()).await?;
            socket.write_all(route.body.as_bytes()).await?;
        }
        Some(slice) => {
            let head = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                route.status
            );
            socket.write_all(head.as_bytes()).await?;
            for piece in route.body.as_bytes().chunks(slice) {
                socket
                    .write_all(format!("{:x}\r\n", piece.len()).as_bytes())
                    .await?;
                socket.write_all(piece).await?;
                socket.write_all(b"\r\n").await?;
                socket.flush().await?;
            }
            socket.write_all(b"0\r\n\r\n").await?;
        }
    }
    socket.flush().await
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .expect("client dựng được")
}

async fn collect(
    adapter: &dyn LlmAdapter,
    req: ChatRequest,
) -> (BlockAssembler, Option<pai_llm::error::LlmError>) {
    let mut stream = adapter.stream(req);
    let mut assembler = BlockAssembler::new();
    let mut failure = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => assembler.push(&chunk),
            Err(err) => {
                failure = Some(err);
                break;
            }
        }
    }
    (assembler, failure)
}

// --- Ollama --------------------------------------------------------------------------

#[tokio::test]
async fn ollama_chat_streaming_qua_http_that() {
    let body = concat!(
        "{\"message\":{\"role\":\"assistant\",\"thinking\":\"cân nhắc\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\"Đã đọc tệp 📄\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",",
        "\"prompt_eval_count\":40,\"eval_count\":7}\n"
    );
    // Khung 6 byte: mọi dòng NDJSON đều bị cắt, và ký tự tiếng Việt cũng vậy.
    let base = serve(vec![("/api/chat", Route::streamed(body, 6))]).await;
    let adapter = OllamaAdapter::new("local", &base, client());

    let (assembler, failure) = collect(
        &adapter,
        ChatRequest::new("qwen3:8b")
            .with_messages(vec![
                Message::system("bạn là trợ lý"),
                Message::user("đọc tệp đi"),
            ])
            .with_keep_alive("5m"),
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(assembler.text(), "Đã đọc tệp 📄");
    assert_eq!(assembler.finish_reason(), Some(FinishReason::Stop));
    assert_eq!(assembler.usage().map(|u| u.total()), Some(47));
}

#[tokio::test]
async fn ollama_loi_http_thanh_ma_loi() {
    let base = serve(vec![(
        "/api/chat",
        Route::status(404, "{\"error\":\"model không tồn tại\"}"),
    )])
    .await;
    let adapter = OllamaAdapter::new("local", &base, client());
    let (_, failure) = collect(&adapter, ChatRequest::new("khong-co")).await;
    let failure = failure.expect("phải hỏng");
    assert_eq!(failure.code, LlmErrorCode::ProviderUnavailable);
    assert_eq!(failure.status, Some(404));
}

/// Kết nối đóng giữa câu: luồng phải kết thúc bằng `Err`, không phải im lặng.
#[tokio::test]
async fn ollama_dut_giua_chung_thanh_err() {
    let base = serve(vec![(
        "/api/chat",
        Route::streamed("{\"message\":{\"content\":\"nua\"},\"done\":false}\n", 4),
    )])
    .await;
    let adapter = OllamaAdapter::new("local", &base, client());
    let (assembler, failure) = collect(&adapter, ChatRequest::new("m")).await;
    assert_eq!(assembler.text(), "nua", "phần đã nhận vẫn phải giữ được");
    assert!(!assembler.is_finished());
    assert_eq!(
        failure.expect("phải hỏng").code,
        LlmErrorCode::ProviderUnavailable
    );
}

/// `/api/show` là nguồn có thẩm quyền: nó nói mô hình gọi được tool, dù cái tên không hé
/// lộ gì cả.
#[tokio::test]
async fn ollama_nang_luc_lay_tu_api_show() {
    let base = serve(vec![
        (
            "/api/tags",
            Route::ok(
                json!({"models": [{
                    "name": "cong-ty-cua-toi:latest",
                    "size": 4_000_000_000u64,
                    "modified_at": "2026-01-02T03:04:05Z",
                    "details": {"family": "llama", "quantization_level": "Q4_K_M"}
                }]})
                .to_string(),
            ),
        ),
        (
            "/api/ps",
            Route::ok(
                json!({"models": [{
                    "name": "cong-ty-cua-toi:latest", "size": 4_000_000_000u64, "size_vram": 5_000_000_000u64
                }]})
                .to_string(),
            ),
        ),
        (
            "/api/show",
            Route::ok(
                json!({
                    "capabilities": ["completion", "tools", "vision", "khong-biet-la-gi"],
                    "model_info": {"llama.context_length": 131072},
                    "details": {"family": "llama", "quantization_level": "Q4_K_M"}
                })
                .to_string(),
            ),
        ),
    ])
    .await;
    let adapter = OllamaAdapter::new("local", &base, client());
    let admin = adapter.admin().expect("Ollama có nửa vòng đời");

    let models = admin.list().await.expect("liệt kê được");
    assert_eq!(models.len(), 1);
    let model = &models[0];
    assert_eq!(model.state, ModelState::Loaded);
    assert_eq!(model.vram_bytes, 5_000_000_000);
    // `completion` được đổi tên thành `chat`; từ vựng lạ bị bỏ.
    assert_eq!(model.capabilities.names(), vec!["chat", "vision", "tools"]);
    assert_eq!(model.capabilities.context_window, Some(131_072));
    assert_eq!(
        model.capabilities.source,
        pai_llm::CapabilitySource::Reported
    );
    // Đo được VRAM thì lấy số đo, không nhân hệ số lên kích thước tệp.
    assert_eq!(model.required_bytes(1.1), 5_000_000_000);

    let capabilities = adapter
        .capabilities("cong-ty-cua-toi:latest")
        .await
        .expect("hỏi được");
    assert!(capabilities.tools);
}

/// `/api/show` không trả lời (bản Ollama cũ): rơi xuống đoán theo tên — và **chỉ** khi ấy.
#[tokio::test]
async fn ollama_api_show_im_lang_thi_doan_theo_ten() {
    let base = serve(vec![
        (
            "/api/tags",
            Route::ok(
                json!({"models": [
                    {"name": "llava:7b", "size": 4_700_000_000u64, "details": {"family": "clip"}},
                    {"name": "nomic-embed-text:latest", "size": 274_000_000u64, "details": {}}
                ]})
                .to_string(),
            ),
        ),
        ("/api/ps", Route::ok(json!({"models": []}).to_string())),
        ("/api/show", Route::status(404, "{}")),
    ])
    .await;
    let adapter = OllamaAdapter::new("local", &base, client());
    let admin = adapter.admin().expect("có admin");
    let models = admin.list().await.expect("liệt kê được");

    assert_eq!(models[0].capabilities.names(), vec!["chat", "vision"]);
    assert_eq!(
        models[0].capabilities.source,
        pai_llm::CapabilitySource::Inferred
    );
    assert_eq!(models[0].state, ModelState::Unloaded);
    // Không đo được VRAM: kích thước tệp cộng biên 10%.
    assert_eq!(models[0].required_bytes(1.1), 5_170_000_000);
    // "embed" thắng trước mọi dấu hiệu khác.
    assert_eq!(models[1].capabilities.names(), vec!["embedding"]);
    assert!(models[1].capabilities.is_embedding_only());
}

#[tokio::test]
async fn ollama_pull_phat_tien_trinh_ndjson() {
    let body = concat!(
        "{\"status\":\"pulling manifest\"}\n",
        "{\"status\":\"pulling 8934d96d\",\"digest\":\"sha256:8934\",\"total\":100,\"completed\":25}\n",
        "{\"status\":\"success\"}\n"
    );
    let base = serve(vec![("/api/pull", Route::streamed(body, 5))]).await;
    let adapter = OllamaAdapter::new("local", &base, client());
    let admin = adapter.admin().expect("có admin");

    let events: Vec<_> = admin.pull("qwen3:8b").collect().await;
    let events: Vec<_> = events
        .into_iter()
        .map(|e| e.expect("dòng hợp lệ"))
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].status, "pulling manifest");
    assert!((events[1].fraction() - 0.25).abs() < f32::EPSILON);
    // Dòng không có số thì tiến trình là 0, không phải "không biết".
    assert_eq!(events[0].fraction(), 0.0);
    assert_eq!(events[2].status, "success");
}

// --- OpenAI-compatible ---------------------------------------------------------------

#[tokio::test]
async fn openai_sse_qua_http_that_voi_tool_call() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        ": ping\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_x\",",
        "\"function\":{\"name\":\"grep\",\"arguments\":\"{\\\"q\\\":\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,",
        "\"function\":{\"arguments\":\"tiếng Việt\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    // Khung 3 byte: mọi event SSE bị cắt nhiều lần, kể cả giữa `\n\n`.
    let base = serve(vec![("/v1/chat/completions", Route::streamed(body, 3))]).await;
    let adapter = OpenAiAdapter::new("cloud", &base, "", client()).expect("dựng được");

    let (assembler, failure) = collect(
        &adapter,
        ChatRequest::new("gpt-oss")
            .with_messages(vec![Message::user("tìm giúp")])
            .with_tools(vec![ToolSchema::new(
                "grep",
                "tìm chuỗi",
                json!({"type": "object"}),
            )]),
    )
    .await;

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(assembler.finish_reason(), Some(FinishReason::ToolCalls));
    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_x");
    assert_eq!(
        calls[0].parse_arguments().expect("JSON hợp lệ")["q"],
        "tiếng Việt"
    );
}

/// Base URL trần được thêm `/v1`; base URL đã có `/v1` thì giữ nguyên. Cả hai phải trỏ
/// vào cùng một chỗ, nếu không người dùng gõ đúng theo cách của mình vẫn hỏng.
#[tokio::test]
async fn openai_chap_nhan_ca_hai_dang_base_url() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let base = serve(vec![("/v1/chat/completions", Route::streamed(body, 8))]).await;

    for url in [base.clone(), format!("{base}/v1"), format!("{base}/v1/")] {
        let adapter = OpenAiAdapter::new("x", &url, "khoa-bi-mat", client()).expect("dựng được");
        let (assembler, failure) = collect(&adapter, ChatRequest::new("m")).await;
        assert!(failure.is_none(), "{url}: {failure:?}");
        assert_eq!(assembler.text(), "ok", "{url}");
    }
}

#[tokio::test]
async fn openai_401_thanh_ma_auth() {
    let base = serve(vec![(
        "/v1/chat/completions",
        Route::status(401, "{\"error\":{\"message\":\"sai khoá\"}}"),
    )])
    .await;
    let adapter = OpenAiAdapter::new("cloud", &base, "sai", client()).expect("dựng được");
    let (_, failure) = collect(&adapter, ChatRequest::new("m")).await;
    assert_eq!(failure.expect("phải hỏng").code, LlmErrorCode::Auth);
}

/// Tràn cửa sổ ngữ cảnh về dưới dạng 400, y hệt một request sai cú pháp. Đây là chỗ duy
/// nhất được phép ngó vào câu chữ, và nó phải hoạt động.
#[tokio::test]
async fn openai_tran_ngu_canh_co_ma_rieng() {
    let base = serve(vec![(
        "/v1/chat/completions",
        Route::status(
            400,
            "{\"error\":{\"message\":\"This model's maximum context length is 8192 tokens\"}}",
        ),
    )])
    .await;
    let adapter = OpenAiAdapter::new("cloud", &base, "", client()).expect("dựng được");
    let (_, failure) = collect(&adapter, ChatRequest::new("m")).await;
    assert_eq!(
        failure.expect("phải hỏng").code,
        LlmErrorCode::ContextWindowExceeded
    );
}

#[tokio::test]
async fn openai_khong_co_nua_vong_doi() {
    let adapter =
        OpenAiAdapter::new("cloud", "http://127.0.0.1:1/v1", "", client()).expect("dựng được");
    assert!(
        adapter.admin().is_none(),
        "mô hình nằm ở nơi khác thì không có gì để nhả"
    );
}

#[tokio::test]
async fn openai_liet_ke_mo_hinh_doan_nang_luc_theo_ten() {
    let base = serve(vec![(
        "/v1/models",
        Route::ok(
            json!({"data": [
                {"id": "text-embedding-3-small", "owned_by": "openai"},
                {"id": "gpt-4o-mini", "owned_by": "openai"}
            ]})
            .to_string(),
        ),
    )])
    .await;
    let adapter = OpenAiAdapter::new("cloud", &base, "", client()).expect("dựng được");
    let models = adapter.list_models().await.expect("liệt kê được");
    // Sắp theo tên.
    assert_eq!(models[0].name, "gpt-4o-mini");
    assert_eq!(models[0].capabilities.names(), vec!["chat", "vision"]);
    assert_eq!(models[1].capabilities.names(), vec!["embedding"]);
    assert_eq!(models[0].state, ModelState::Installed);
}
