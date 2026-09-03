//! Kho mô hình của một provider, hỏi qua HTTP thật.
//!
//! Bài này canh đúng một thứ: cờ `embedding` của từng mô hình **đến từ máy chủ**, không
//! đến từ một cái tên đặt sẵn trong mã. Trước đây màn hình mô hình nhúng điền sẵn
//! `nomic-embed-text` cho mọi máy chủ Ollama, và một người đã kéo `bge-m3` về sẽ không
//! thấy nó ở đâu cả — nên hai mô hình được chọn ở đây đều là mô hình nhúng **không có chữ
//! "embed" trong tên**. Đoán theo tên trượt cả hai; hỏi máy chủ thì không.
//!
//! Máy chủ giả dựng bằng `TcpListener` thuần, cùng lối với `pai-llm/tests/adapters_http.rs`:
//! bộ test phải chạy được khi rút dây mạng.

use std::sync::Arc;

use pai_agent::{Driver, SystemPrompt};
use pai_core::Context;
use pai_llm::{AdapterRegistry, ProviderConfig, ProviderKind};
use pai_providers::{ProviderRuntime, SqliteProviderStore};
use pai_tools::{ToolPipeline, ToolRegistry};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Máy chủ HTTP tối giản: một hàm quyết định thân trả về từ đường dẫn và thân nhận được.
///
/// Nhận cả thân request vì `/api/show` của Ollama là một `POST` — cùng một đường dẫn cho
/// mọi mô hình, và cái phân biệt chúng nằm trong thân.
async fn serve(reply: impl Fn(&str, &str) -> Value + Send + Sync + 'static) -> String {
    let reply = Arc::new(reply);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("địa chỉ đã gán");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let reply = reply.clone();
            tokio::spawn(async move {
                let Some((path, body)) = read_request(&mut socket).await else {
                    return;
                };
                let payload = reply(&path, &body).to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(payload.as_bytes()).await;
                let _ = socket.flush().await;
            });
        }
    });
    format!("http://{addr}")
}

/// Đọc trọn request. Thân phải đọc hết dù có dùng tới hay không: bỏ dở thì client thấy
/// một cú reset chứ không thấy phản hồi.
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

fn runtime() -> ProviderRuntime {
    let ctx = Context::root();
    let tools = ToolRegistry::new(&ctx);
    let pipeline = Arc::new(ToolPipeline::new(&ctx, tools));
    let http = reqwest::Client::new();
    let registry = Arc::new(AdapterRegistry::new(http.clone()));
    let store = Arc::new(SqliteProviderStore::open_in_memory().expect("mở kho"));
    let driver = Arc::new(Driver::new(
        ctx,
        registry
            .adapter(&pai_providers::PRESETS[0].config())
            .expect("adapter khởi điểm"),
        pipeline,
        SystemPrompt::new(),
        "chua-chon",
    ));
    ProviderRuntime::new(store, registry, driver, http)
}

#[tokio::test]
async fn ollama_khai_mo_hinh_nao_nhung_duoc_thi_lay_dung_cai_do() {
    let base = serve(|path, body| match path {
        "/api/tags" => json!({"models": [
            {"name": "bge-m3:latest", "size": 1_200_000_000u64, "details": {}},
            {"name": "qwen2.5:7b", "size": 4_700_000_000u64, "details": {}}
        ]}),
        "/api/ps" => json!({"models": []}),
        // `/api/show` là nguồn có thẩm quyền — và ở đây nó nói ngược hẳn với cái tên.
        "/api/show" if body.contains("bge-m3") => json!({"capabilities": ["embedding"]}),
        "/api/show" => json!({"capabilities": ["completion", "tools"]}),
        _ => json!({}),
    })
    .await;

    let config = ProviderConfig::new("pv", "Ollama nhà", ProviderKind::Ollama, base);
    let models = runtime().models(&config).await;

    let bge = models.iter().find(|m| m.id == "bge-m3:latest").expect("có bge-m3");
    assert!(bge.embedding, "máy chủ khai nhúng được mà danh sách nói không");
    let qwen = models.iter().find(|m| m.id == "qwen2.5:7b").expect("có qwen");
    assert!(!qwen.embedding, "mô hình trò chuyện không được trôi vào nhóm nhúng");
    assert!(qwen.chat && qwen.tools);
}

#[tokio::test]
async fn provider_tu_xa_khong_co_nua_vong_doi_thi_van_liet_ke_duoc() {
    // Không có `ModelAdmin` để hỏi, nên lõi rơi xuống nhánh liệt kê và đoán theo tên.
    // Danh sách đoán vẫn hơn hẳn một ô trống — nhưng nó *là* phỏng đoán, và đó đúng là lý
    // do màn hình phải giữ lối gõ tay.
    let base = serve(|path, _| match path {
        "/v1/models" => json!({"data": [
            {"id": "text-embedding-3-small"},
            {"id": "gpt-4o-mini"}
        ]}),
        _ => json!({}),
    })
    .await;

    let config = ProviderConfig::new(
        "pv",
        "Máy chủ nội bộ",
        ProviderKind::OpenAiCompatible,
        format!("{base}/v1"),
    )
    .with_api_key("sk-thu");
    let models = runtime().models(&config).await;

    assert_eq!(models.len(), 2);
    assert!(models.iter().any(|m| m.id == "text-embedding-3-small" && m.embedding));
    assert!(models.iter().any(|m| m.id == "gpt-4o-mini" && !m.embedding));
}

#[tokio::test]
async fn lm_studio_khai_thang_loai_mo_hinh_nen_khong_phai_doan_ten() {
    // `/api/v0/models` mang sẵn `type` cho từng mục, nên đây là loại provider duy nhất mà
    // cờ `embedding` là **sự thật** chứ không phải phỏng đoán — và nó nói đúng cái mà một
    // phép đoán theo tên nói sai: `text-embedding-nomic-embed-text-v1.5` là mô hình nhúng,
    // còn `openai/gpt-oss-120b` thì không.
    let base = serve(|path, _| match path {
        "/api/v0/models" => json!({"data": [
            {"id": "text-embedding-nomic-embed-text-v1.5", "type": "embeddings",
             "state": "not-loaded"},
            {"id": "openai/gpt-oss-120b", "type": "llm", "state": "loaded",
             "max_context_length": 131_072u64}
        ]}),
        _ => json!({}),
    })
    .await;

    // URL có đuôi `/v1` như người dùng hay dán vào: adapter tự tìm về gốc máy chủ.
    let config = ProviderConfig::new(
        "pv",
        "LM Studio",
        ProviderKind::LmStudio,
        format!("{base}/v1"),
    );
    let models = runtime().models(&config).await;

    let embed = models
        .iter()
        .find(|m| m.id == "text-embedding-nomic-embed-text-v1.5")
        .expect("có mô hình nhúng");
    assert!(embed.embedding && !embed.chat);
    let chat = models
        .iter()
        .find(|m| m.id == "openai/gpt-oss-120b")
        .expect("có mô hình hội thoại");
    assert!(chat.chat && !chat.embedding);
}

#[tokio::test]
async fn may_chu_khong_tra_loi_thi_danh_sach_rong_chu_khong_phai_mot_cai_ten_bia_ra() {
    // Cổng không có ai nghe. Rỗng là câu trả lời đúng: nó nói "không hỏi được", và giao
    // diện đọc nó thành "mở ô gõ tay ra" — chứ không phải điền sẵn một mô hình có thể
    // không tồn tại trên máy này.
    let config = ProviderConfig::new(
        "pv",
        "Ollama chưa bật",
        ProviderKind::Ollama,
        "http://127.0.0.1:1",
    );
    assert!(runtime().models(&config).await.is_empty());
}
