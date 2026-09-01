//! Hai vai trên một danh sách provider.
//!
//! Bài quan trọng nhất ở đây là [`hai_vai_tro_vao_hai_provider_khac_nhau`]: nó là toàn bộ
//! lý do của việc tách vai. Bài còn lại canh hai chỗ hỏng đắt tiền — một lần migrate làm
//! bay khoá API của người dùng, và một con trỏ còn trỏ vào hàng vừa bị xoá.
//!
//! Không bài nào chạm mạng.

use pai_llm::ProviderKind;
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};
use rusqlite::Connection;

fn them(store: &SqliteProviderStore, name: &str, kind: ProviderKind, url: &str) -> StoredProvider {
    store
        .save(ProviderInput::create(name, kind, url))
        .expect("lưu")
}

/// Lược đồ đúng như bản trước khi có hai vai: một cột trạng thái duy nhất, và bảng
/// `providers` chưa có `embedding_model`.
const LUOC_DO_CU: &str = "
CREATE TABLE providers (
  id         TEXT    PRIMARY KEY,
  name       TEXT    NOT NULL,
  kind       TEXT    NOT NULL,
  base_url   TEXT    NOT NULL,
  api_key    TEXT    NOT NULL,
  enabled    INTEGER NOT NULL,
  model      TEXT,
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE provider_state (
  id        INTEGER PRIMARY KEY CHECK (id = 0),
  active_id TEXT REFERENCES providers (id) ON DELETE SET NULL
) STRICT;

INSERT INTO providers VALUES
  ('mot', 'Ollama nhà', 'ollama', 'http://localhost:11434', '', 1, 'qwen3:8b', 1),
  ('hai', 'OpenAI', 'openai', 'https://api.openai.com/v1', 'sk-khoa-that-cua-nguoi-dung', 1,
   'gpt-4o-mini', 2);

INSERT INTO provider_state (id, active_id) VALUES (0, 'hai');
";

#[test]
fn tep_cua_ban_cu_len_lam_vai_hoi_thoai_va_khong_mat_khoa() {
    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join(pai_providers::DB_FILE);
    {
        let conn = Connection::open(&path).expect("dựng tệp cũ");
        conn.execute_batch(LUOC_DO_CU).expect("lược đồ cũ");
    }

    let store = SqliteProviderStore::open(&path).expect("mở kho đã nâng cấp");
    let rows = store.list().expect("liệt kê");

    // Hàng còn nguyên, **kể cả khoá**: đây là thứ người dùng gõ vào và không lấy lại được
    // nếu mất.
    assert_eq!(rows.len(), 2, "{rows:?}");
    let openai = rows.iter().find(|row| row.id() == "hai").expect("còn hàng");
    assert_eq!(openai.config.api_key, "sk-khoa-that-cua-nguoi-dung");
    assert_eq!(openai.config.name, "OpenAI");
    assert_eq!(openai.model.as_deref(), Some("gpt-4o-mini"));

    // Hàng đang hoạt động của bản cũ luôn là hàng dùng để trò chuyện.
    assert!(openai.active_chat);
    assert_eq!(
        store
            .active(Role::Chat)
            .expect("đọc")
            .map(|row| row.id().to_string()),
        Some("hai".to_string())
    );

    // Vai nhúng bắt đầu từ chỗ trống: người dùng chưa từng được hỏi tài liệu của họ sẽ
    // được gửi đi đâu, nên không ai được trả lời thay.
    assert!(rows.iter().all(|row| !row.active_embedding), "{rows:?}");
    assert!(store.active(Role::Embedding).expect("đọc").is_none());
    assert!(rows.iter().all(|row| row.embedding_model.is_none()));
}

#[test]
fn hai_vai_tro_vao_hai_provider_khac_nhau() {
    let store = SqliteProviderStore::open_in_memory().expect("mở kho");
    let ollama = them(
        &store,
        "Ollama nhà",
        ProviderKind::Ollama,
        "http://localhost:11434",
    );
    let openai = them(
        &store,
        "OpenAI",
        ProviderKind::OpenAiCompatible,
        "https://api.openai.com/v1",
    );

    // Ghép chéo: nhúng tại chỗ, trò chuyện từ xa.
    store
        .activate(Role::Embedding, ollama.id(), Some("nomic-embed-text"))
        .expect("trao vai nhúng");
    store
        .activate(Role::Chat, openai.id(), Some("gpt-4o-mini"))
        .expect("trao vai hội thoại");

    let chat = store
        .active(Role::Chat)
        .expect("đọc")
        .expect("có vai hội thoại");
    let nhung = store
        .active(Role::Embedding)
        .expect("đọc")
        .expect("có vai nhúng");
    assert_eq!(chat.id(), openai.id());
    assert_eq!(nhung.id(), ollama.id());
    assert_ne!(chat.id(), nhung.id());

    // Mỗi vai ghi vào cột mô hình của riêng nó. Trộn hai cột lại là gửi `gpt-4o-mini` tới
    // `/api/embed`, hoặc `nomic-embed-text` vào một lượt trò chuyện.
    assert_eq!(chat.model.as_deref(), Some("gpt-4o-mini"));
    assert_eq!(chat.embedding_model, None);
    assert_eq!(nhung.embedding_model.as_deref(), Some("nomic-embed-text"));
    assert!(nhung.active_embedding && !nhung.active_chat);
    assert!(chat.active_chat && !chat.active_embedding);
}

#[test]
fn xoa_provider_giu_ca_hai_vai_thi_khong_vai_nao_con_tro_vao_id_chet() {
    let store = SqliteProviderStore::open_in_memory().expect("mở kho");
    let ca_hai = them(
        &store,
        "Một mình một chợ",
        ProviderKind::Ollama,
        "http://localhost:11434",
    );
    let con_lai = them(
        &store,
        "Dự bị",
        ProviderKind::OpenAiCompatible,
        "http://localhost:1234/v1",
    );
    store
        .activate(Role::Chat, ca_hai.id(), Some("qwen3:8b"))
        .expect("vai hội thoại");
    store
        .activate(Role::Embedding, ca_hai.id(), Some("nomic-embed-text"))
        .expect("vai nhúng");

    store.remove(ca_hai.id()).expect("xoá");

    // Hội thoại có người kế nhiệm — ứng dụng phải trả lời được. Nhúng thì không: gửi tài
    // liệu tới một máy chủ người dùng chưa chọn còn tệ hơn là không nhúng.
    let chat = store
        .active(Role::Chat)
        .expect("đọc")
        .expect("phải còn ai đó trò chuyện");
    assert_eq!(chat.id(), con_lai.id());
    assert!(store.active(Role::Embedding).expect("đọc").is_none());
    assert!(
        store
            .list()
            .expect("liệt kê")
            .iter()
            .all(|row| !row.active_embedding)
    );
}
