//! The embedding role: who holds it, and the explanation when it is not usable. Endpoint tests moved to
//! `services/rag/`; what remains is the store's job -- holding the role, remembering the model, and saying
//! why embedding is unavailable, since three causes need three different sentences. Nothing leaves the machine.

use std::net::TcpListener;

use pai_llm::{ProviderConfig, ProviderKind};
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};

/// A just-released loopback port: guaranteed nobody is listening.
fn cong_dong() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mượn cổng");
    let port = listener.local_addr().expect("địa chỉ").port();
    drop(listener);
    port
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
    // Holding the role with no model writes an empty `model` rather than borrowing the chat one, and the layer above must explain why.
    let ly_do = pai_providers::embedding_reason(Some(&chua_chon)).expect("phải có lý do");
    assert!(ly_do.contains("qwen3-embedding"), "{ly_do}");
    assert!(
        pai_providers::embedding_reason(None)
            .expect("chưa ai giữ vai cũng phải có lý do")
            .contains("Chưa chọn"),
    );
    assert!(
        pai_providers::embedding_reason(Some(&hang(
            ProviderKind::Ollama,
            "http://localhost:11434",
            Some("qwen3-embedding:4b")
        )))
        .is_none()
    );
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
        result.message.contains("Không kết nối được"),
        "phải thuộc nhóm không nối được: {}",
        result.message
    );
    // The bad-key case calls for a different user action; mentioning the key here sends them to fix the wrong thing.
    assert!(
        !result.message.to_lowercase().contains("khoá"),
        "không được đổ cho khoá: {}",
        result.message
    );
    assert!(result.dimensions.is_none());
}
