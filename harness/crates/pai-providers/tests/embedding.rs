//! Vai nhúng: ai giữ nó, và câu giải thích khi nó chưa dùng được.
//!
//! Bài kiểm "gõ vào endpoint nào" từng ở đây đã đi cùng phần nhúng sang
//! `services/rag/` — xem `tests/` của gói Python. Còn lại ở đây là phần mà kho provider
//! vẫn chịu trách nhiệm: giữ vai, nhớ tên mô hình, và **nói ra vì sao chưa nhúng được**.
//!
//! Câu giải thích ấy là thứ đáng kiểm nhất trong tệp này. Nó là chỗ duy nhất người dùng
//! biết được vì sao thư viện của họ chỉ tìm theo từ khoá, và ba nguyên nhân khác nhau —
//! chưa ai giữ vai, provider bị tắt, chưa chọn mô hình — cần ba câu khác nhau.
//!
//! Không có gì ra khỏi máy này: một cổng loopback vừa nhả ra, và một `TcpListener` tự viết.

use std::net::TcpListener;

use pai_llm::{ProviderConfig, ProviderKind};
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};

/// Một cổng loopback vừa được nhả ra: chắc chắn không có ai nghe.
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
    // Giữ vai mà chưa chọn mô hình thì tệp cấu hình ghi ra một `model` **rỗng**, chứ
    // không mượn tạm `model` của vai hội thoại: `qwen3:8b` không có endpoint embed, và
    // mọi lần nạp tài liệu sẽ trả 400.
    // Tầng trên phải nói ra được vì sao.
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
