//! Ba adapter, chạy qua HTTP thật — nhưng với **một máy chủ dựng ngay trong bài test**.
//!
//! Không `wiremock`, không mạng: một `TcpListener` của tokio và vài chục dòng nói HTTP/1.1
//! là đủ, và nó cho ta thứ mà thư viện giả lập nào cũng giấu — quyền quyết định **byte
//! nào đi cùng gói nào**. Mọi phản hồi streaming ở đây được gửi theo `Transfer-Encoding:
//! chunked` với khung cố tình cắt lệch, nên đường ống thật phải chịu đúng cái cắt vụn mà
//! bài test đơn vị mô tả.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use pai_llm::assembler::BlockAssembler;
use pai_llm::error::LlmErrorCode;
use pai_llm::message::{ChatRequest, Message, ToolSchema};
use pai_llm::model::ModelState;
use pai_llm::lmstudio::LmStudioAdapter;
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
    serve_recording(routes).await.0
}

/// Như [`serve`], nhưng giữ lại mọi request đã tới.
async fn serve_recording(routes: Vec<(&str, Route)>) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let table: HashMap<String, Route> = routes
        .into_iter()
        .map(|(path, route)| (path.to_string(), route))
        .collect();
    let table = Arc::new(table);
    let log: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = log.clone();
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
            let sink = sink.clone();
            tokio::spawn(async move {
                let Some((path, body)) = read_request(&mut socket).await else {
                    return;
                };
                // Ghi lại **trước** khi trả lời: một bài kiểm chứng về thân request phải
                // đọc được nó kể cả khi tuyến đường không tồn tại và máy chủ trả 404.
                sink.lock().expect("khoá sổ").push((path.clone(), body));
                let route = table
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| Route::status(404, "{\"error\":\"không có đường này\"}"));
                let _ = write_response(&mut socket, &route).await;
            });
        }
    });
    (format!("http://{addr}"), log)
}

/// Đọc trọn request, trả về đường dẫn **và thân**. Thân phải được đọc hết dù bài kiểm
/// chứng có cần tới nó hay không: bỏ dở thì client thấy một cú reset chứ không thấy phản
/// hồi.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<(String, String)> {
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
    let path = head.lines().next()?.split_whitespace().nth(1)?.to_string();
    let body = String::from_utf8_lossy(&buffer[head_end..]).to_string();
    Some((path, body))
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

// --- LM Studio -----------------------------------------------------------------------

#[tokio::test]
async fn lmstudio_chat_sse_va_ttl_di_theo() {
    // Đúng hình dạng SSE của OpenAI, vì LM Studio nói đúng giao thức ấy. Bài này khoá
    // điều đó lại: nếu ai đó tách bộ giải mã ra thành một bản riêng cho LM Studio thì
    // nó phải tiếp tục đi qua đây.
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Xin \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"chào 👋\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":4}}\n\n",
        "data: [DONE]\n\n"
    );
    let (base, log) = serve_recording(vec![(
        "/v1/chat/completions",
        Route::streamed(body, 7),
    )])
    .await;
    let adapter = LmStudioAdapter::new("lms", &base, "", client());

    let (assembler, failure) = collect(
        &adapter,
        ChatRequest::new("qwen3-8b")
            .with_messages(vec![Message::user("chào")])
            // `keep_alive` là từ vựng Ollama. Adapter OpenAI bỏ qua nó; LM Studio thì
            // hiểu cùng khái niệm ấy dưới tên `ttl`, đo bằng giây.
            .with_keep_alive("5m"),
    )
    .await;

    assert!(failure.is_none(), "không được hỏng: {failure:?}");
    assert_eq!(assembler.text(), "Xin chào 👋");
    assert_eq!(assembler.finish_reason(), Some(FinishReason::Stop));

    let sent = log.lock().expect("khoá sổ");
    let (path, body) = sent.first().expect("có đúng một request");
    assert_eq!(path, "/v1/chat/completions");
    let sent: serde_json::Value = serde_json::from_str(body).expect("thân là JSON");
    assert_eq!(sent["ttl"], json!(300), "`5m` phải thành 300 giây");
    // Và `keep_alive` **không** được lọt sang: LM Studio không biết trường ấy.
    assert!(sent.get("keep_alive").is_none());
}

#[tokio::test]
async fn lmstudio_nang_luc_doc_tu_api_v0_chu_khong_doan_theo_ten() {
    // `nha-cua-toi-v3` là một bản fine-tune tự đặt tên: đoán theo tên không rút ra được
    // gì từ nó. Đây đúng là chỗ `/v1/models` bó tay và `/api/v0/models` trả lời được.
    let base = serve(vec![(
        "/api/v0/models/nha-cua-toi-v3",
        Route::ok(
            r#"{"id":"nha-cua-toi-v3","type":"llm","arch":"qwen3","quantization":"Q4_K_M",
                "state":"loaded","max_context_length":32768,"loaded_context_length":8192,
                "capabilities":["tool_use","vision"]}"#,
        ),
    )])
    .await;
    let adapter = LmStudioAdapter::new("lms", &base, "", client());

    let caps = adapter
        .capabilities("nha-cua-toi-v3")
        .await
        .expect("hỏi được");
    assert!(caps.tools, "máy chủ khai tool_use");
    assert!(caps.vision);
    assert!(caps.chat);
    assert_eq!(caps.source, pai_llm::CapabilitySource::Reported);
    // Cửa sổ **đang nạp** thắng cửa sổ tối đa: đó là con số thật của lượt sắp chạy.
    assert_eq!(caps.context_window, Some(8192));
}

#[tokio::test]
async fn lmstudio_khong_hoi_duoc_thi_doan_theo_ten_chu_khong_hong() {
    // Máy chủ đời cũ, không có `/api/v0`. Một lượt vẫn phải chạy được.
    let base = serve(vec![]).await;
    let adapter = LmStudioAdapter::new("lms", &base, "", client());

    let caps = adapter.capabilities("llava-7b").await.expect("không hỏng");
    assert_eq!(caps.source, pai_llm::CapabilitySource::Inferred);
    assert!(caps.vision, "`llava` đoán được là mô hình thị giác");
}

#[tokio::test]
async fn lmstudio_danh_sach_biet_mo_hinh_nao_dang_nam_trong_vram() {
    let base = serve(vec![(
        "/api/v0/models",
        Route::ok(
            r#"{"object":"list","data":[
                {"id":"qwen3-8b","type":"llm","state":"loaded","max_context_length":40960,
                 "quantization":"Q4_K_M","arch":"qwen3"},
                {"id":"nomic-embed-text-v1.5","type":"embeddings","state":"not-loaded",
                 "max_context_length":2048},
                {"id":"gemma-3-4b","type":"vlm","state":"not-loaded","max_context_length":8192}
            ]}"#,
        ),
    )])
    .await;
    let adapter = LmStudioAdapter::new("lms", &base, "", client());
    let admin = adapter.admin().expect("LM Studio có nửa vòng đời");

    let models = admin.list().await.expect("liệt kê được");
    let by_name = |name: &str| {
        models
            .iter()
            .find(|model| model.name == name)
            .unwrap_or_else(|| panic!("thiếu {name}"))
            .clone()
    };

    assert_eq!(by_name("qwen3-8b").state, ModelState::Loaded);
    assert_eq!(by_name("gemma-3-4b").state, ModelState::Unloaded);
    // Phân loại mô hình nhúng là thứ `/v1/models` không làm được, và là chỗ người dùng
    // chọn nhầm nhiều nhất.
    let embed = by_name("nomic-embed-text-v1.5");
    assert!(embed.capabilities.is_embedding_only());
    assert!(by_name("gemma-3-4b").capabilities.vision, "`vlm` là thị giác");
    assert_eq!(by_name("qwen3-8b").quantization.as_deref(), Some("Q4_K_M"));

    let running = admin.running().await.expect("hỏi được");
    assert_eq!(
        running.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        vec!["qwen3-8b"]
    );
}

#[tokio::test]
async fn lmstudio_ba_dong_tu_khong_co_thi_noi_ra_cach_khac() {
    use futures::StreamExt as _;

    let base = serve(vec![]).await;
    let adapter = LmStudioAdapter::new("lms", &base, "", client());
    let admin = adapter.admin().expect("có nửa vòng đời");

    let pulled = admin.pull("qwen3-8b").next().await.expect("có một dòng");
    let err = pulled.expect_err("LM Studio không tải về qua API");
    assert_eq!(err.code, LlmErrorCode::Unsupported);
    // Câu chữ phải chỉ ra **việc phải làm**, không chỉ nói là không được.
    assert!(err.message.contains("lms get"), "{}", err.message);

    let err = admin.unload("qwen3-8b").await.expect_err("không nhả được");
    assert_eq!(err.code, LlmErrorCode::Unsupported);
    assert!(err.message.contains("lms unload"), "{}", err.message);

    let err = admin.delete("qwen3-8b").await.expect_err("không xoá được");
    assert_eq!(err.code, LlmErrorCode::Unsupported);
}

#[tokio::test]
async fn lmstudio_nhan_ca_ba_dang_base_url_va_ma_hoa_ten_co_gach_cheo() {
    let (base, log) = serve_recording(vec![(
        // Tên mô hình của LM Studio mang dấu `/`. Không mã hoá thì nó tự mọc thêm một
        // đoạn đường dẫn và máy chủ trả 404 — một lỗi không ai đoán ra từ triệu chứng.
        "/api/v0/models/lmstudio-community%2Fqwen3-8b",
        Route::ok(r#"{"id":"lmstudio-community/qwen3-8b","type":"llm","max_context_length":4096}"#),
    )])
    .await;

    for suffix in ["", "/v1", "/api/v0"] {
        let adapter = LmStudioAdapter::new("lms", format!("{base}{suffix}"), "", client());
        assert_eq!(adapter.base_url(), base, "đuôi `{suffix}` phải bị cắt");
        let details = adapter
            .admin()
            .expect("có nửa vòng đời")
            .show("lmstudio-community/qwen3-8b")
            .await
            .expect("hỏi được");
        assert_eq!(details.capabilities.context_window, Some(4096));
    }
    assert_eq!(log.lock().expect("khoá sổ").len(), 3);
}
