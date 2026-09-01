//! Bộ nhúng dựng từ vai nhúng, và phép thử nhúng thật.
//!
//! Câu hỏi mà bài adapter hỏi không phải "có dựng được một `Embedder` không" mà **"nó gõ
//! vào cửa nào"**: hai loại provider có hai endpoint khác nhau, và một `base_url` mang đuôi
//! `/v1` đi nhầm đường sẽ thành `/v1/v1/embeddings`. Nên bài này dựng một máy chủ giả trên
//! loopback và đọc lại đường dẫn thật đã nhận được.
//!
//! Không có gì ra khỏi máy này: một cổng loopback vừa nhả ra, và một `TcpListener` tự viết.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use pai_llm::{ProviderConfig, ProviderKind};
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};

/// Một cổng loopback vừa được nhả ra: chắc chắn không có ai nghe.
fn cong_dong() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mượn cổng");
    let port = listener.local_addr().expect("địa chỉ").port();
    drop(listener);
    port
}

/// Máy chủ giả nhận đúng một request, ghi lại đường dẫn, và trả về một thân hợp lệ cho
/// **cả hai** giao thức — để thứ duy nhất phân biệt được hai adapter là đường dẫn.
fn may_chu_gia(da_nhan: Arc<Mutex<Vec<String>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mượn cổng");
    let port = listener.local_addr().expect("địa chỉ").port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        // Đọc hết cả thân, không chỉ phần đầu: đóng socket khi còn dữ liệu chưa đọc là
        // một RST, và bài kiểm chứng khi đó hỏng vì lý do không liên quan gì tới nó.
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if da_du(&buf) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let head = String::from_utf8_lossy(&buf).to_string();
        let duong_dan = head
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        da_nhan
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(duong_dan);
        let than =
            r#"{"embeddings":[[0.1,0.2,0.3]],"data":[{"index":0,"embedding":[0.1,0.2,0.3]}]}"#;
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{than}",
                than.len()
            )
            .as_bytes(),
        );
        let _ = stream.flush();
    });
    port
}

/// Đã nhận đủ phần đầu và trọn `Content-Length` chưa.
fn da_du(buf: &[u8]) -> bool {
    let text = String::from_utf8_lossy(buf);
    let Some(ranh) = text.find("\r\n\r\n") else {
        return false;
    };
    let can = text[..ranh]
        .lines()
        .find_map(|line| {
            let (ten, gia_tri) = line.split_once(':')?;
            ten.eq_ignore_ascii_case("content-length")
                .then(|| gia_tri.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    buf.len() >= ranh + 4 + can
}

fn hang(kind: ProviderKind, url: &str, embedding_model: Option<&str>) -> StoredProvider {
    let store = SqliteProviderStore::open_in_memory().expect("mở kho");
    let mut input = ProviderInput::create("Thử", kind, url);
    input.api_key = Some("khoa".to_string());
    let saved = store.save(input).expect("lưu");
    store
        .activate(Role::Embedding, saved.id(), embedding_model)
        .expect("trao vai nhúng")
}

#[test]
fn chua_chon_mo_hinh_nhung_thi_khong_co_bo_nhung() {
    let chua_chon = hang(ProviderKind::Ollama, "http://localhost:11434", None);
    assert!(chua_chon.active_embedding, "vẫn giữ vai: {chua_chon:?}");
    // Giữ vai mà chưa chọn mô hình là `None` chứ không phải mượn tạm `model` của vai hội
    // thoại: `qwen3:8b` không có endpoint embed, và mọi lần nạp tài liệu sẽ trả 400.
    assert!(pai_providers::embedder_for(&chua_chon).is_none());

    // Nhưng tầng trên phải nói ra được vì sao.
    let ly_do = pai_providers::embedding_reason(Some(&chua_chon)).expect("phải có lý do");
    assert!(ly_do.contains("nomic-embed-text"), "{ly_do}");
    assert!(
        pai_providers::embedding_reason(None)
            .expect("chưa ai giữ vai cũng phải có lý do")
            .contains("Chưa chọn"),
    );
    assert!(
        pai_providers::embedding_reason(Some(&hang(
            ProviderKind::Ollama,
            "http://localhost:11434",
            Some("nomic-embed-text")
        )))
        .is_none()
    );
}

#[tokio::test]
async fn ollama_go_vao_api_embed_con_openai_go_vao_v1_embeddings() {
    for (kind, duoi_url, mong_doi) in [
        (ProviderKind::Ollama, "", "/api/embed"),
        (ProviderKind::OpenAiCompatible, "/v1", "/v1/embeddings"),
    ] {
        let da_nhan = Arc::new(Mutex::new(Vec::new()));
        let port = may_chu_gia(da_nhan.clone());
        let row = hang(
            kind,
            &format!("http://127.0.0.1:{port}{duoi_url}"),
            Some("mo-hinh-nhung"),
        );

        let embedder = pai_providers::embedder_for(&row).expect("phải dựng được bộ nhúng");
        assert_eq!(embedder.id(), "mo-hinh-nhung");
        let vectors = embedder
            .embed(&["một câu".to_string()])
            .await
            .expect("nhúng");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), 3);

        let da_nhan = da_nhan.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert_eq!(da_nhan, vec![mong_doi.to_string()], "{kind:?}");
    }
}

#[tokio::test]
async fn khong_noi_duoc_thi_dung_do_cho_cai_khoa() {
    let port = cong_dong();
    let config = ProviderConfig::new(
        "thu",
        "Máy chủ chưa bật",
        ProviderKind::OpenAiCompatible,
        format!("http://127.0.0.1:{port}/v1"),
    )
    .with_api_key("khoa-hoan-toan-hop-le");

    let result = pai_providers::probe_embedding(&config, "text-embedding-3-small").await;
    assert!(!result.ok, "không được coi là nhúng được: {result:?}");
    assert!(
        result.message.contains("Không nối được"),
        "phải thuộc nhóm không nối được: {}",
        result.message
    );
    // Sai khoá là một hành động khác hẳn của người dùng. Nhắc tới khoá ở đây là đẩy họ đi
    // sửa nhầm chỗ.
    assert!(
        !result.message.to_lowercase().contains("khoá"),
        "không được đổ cho khoá: {}",
        result.message
    );
    assert!(result.dimensions.is_none());
}
