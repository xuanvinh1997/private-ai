//! Two roles over one provider list. The central test is [`hai_vai_tro_vao_hai_provider_khac_nhau`], the
//! whole reason roles are split; the rest guard two expensive failures -- a migration losing API keys, and
//! a pointer left aimed at a deleted row. Nothing here touches the network.

use pai_llm::ProviderKind;
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};
use rusqlite::Connection;

fn them(store: &SqliteProviderStore, name: &str, kind: ProviderKind, url: &str) -> StoredProvider {
    store
        .save(ProviderInput::create(name, kind, url))
        .expect("lưu")
}

/// The exact pre-roles schema: a single state column, and `providers` without `embedding_model`.
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

    // Rows intact, keys included: this is hand-typed data that cannot be recovered if lost.
    assert_eq!(rows.len(), 2, "{rows:?}");
    let openai = rows.iter().find(|row| row.id() == "hai").expect("còn hàng");
    assert_eq!(openai.config.api_key, "sk-khoa-that-cua-nguoi-dung");
    assert_eq!(openai.config.name, "OpenAI");
    assert_eq!(openai.model.as_deref(), Some("gpt-4o-mini"));

    // The old build's active row was always the chat row.
    assert!(openai.active_chat);
    assert_eq!(
        store
            .active(Role::Chat)
            .expect("đọc")
            .map(|row| row.id().to_string()),
        Some("hai".to_string())
    );

    // The embedding role starts empty: the user was never asked where their documents go, so nobody answers for them.
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

    // Cross-wired: embed locally, chat remotely.
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

    // Each role writes its own model column; mixing them would send `gpt-4o-mini` to `/api/embed`.
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

    // Chat gets a successor because the app must answer; embedding does not, since an unchosen server is worse than none.
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
