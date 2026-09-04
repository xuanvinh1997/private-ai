//! Provider-store invariants. Three of the four tests lock bugs that actually happened before: keys
//! vanishing on a rename, keys leaking into logs, and an active pointer aimed at a deleted row.

use pai_llm::ProviderKind;
use pai_providers::{ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};

fn store() -> SqliteProviderStore {
    SqliteProviderStore::open_in_memory().expect("mở kho")
}

fn them(store: &SqliteProviderStore, name: &str, key: Option<&str>) -> StoredProvider {
    let mut input = ProviderInput::create(
        name,
        ProviderKind::OpenAiCompatible,
        "https://api.openai.com/v1",
    );
    input.api_key = key.map(str::to_string);
    store.save(input).expect("lưu")
}

#[test]
fn khoa_vang_mat_la_giu_nguyen_chu_khong_phai_xoa() {
    let store = store();
    let saved = them(&store, "OpenAI", Some("bi-mat"));

    // `None`: the UI submits a form with no key field, because it never received the key to resend.
    let doi_ten = store
        .save(
            ProviderInput::create(
                "OpenAI nhà",
                ProviderKind::OpenAiCompatible,
                "https://api.openai.com/v1",
            )
            .with_id(saved.id()),
        )
        .expect("đổi tên");
    assert_eq!(doi_ten.config.api_key, "bi-mat");
    assert_eq!(doi_ten.config.name, "OpenAI nhà");

    // `Some("k")`: a real change.
    let doi_khoa = store
        .save(
            ProviderInput::create(
                "OpenAI nhà",
                ProviderKind::OpenAiCompatible,
                "https://api.openai.com/v1",
            )
            .with_id(saved.id())
            .with_api_key("k"),
        )
        .expect("đổi khoá");
    assert_eq!(doi_khoa.config.api_key, "k");

    // `Some("")`: the only way to clear the key.
    let xoa_khoa = store
        .save(
            ProviderInput::create(
                "OpenAI nhà",
                ProviderKind::OpenAiCompatible,
                "https://api.openai.com/v1",
            )
            .with_id(saved.id())
            .with_api_key(""),
        )
        .expect("xoá khoá");
    assert_eq!(xoa_khoa.config.api_key, "");
    assert!(!xoa_khoa.has_key());
}

#[test]
fn khoa_khong_bao_gio_lot_vao_debug() {
    let store = store();
    let saved = them(&store, "OpenAI", Some("sk-khong-duoc-in-ra"));

    let in_ra = format!("{saved:?}");
    assert!(
        !in_ra.contains("sk-khong-duoc-in-ra"),
        "khoá lọt ra Debug: {in_ra}"
    );
    assert!(in_ra.contains("<đã đặt>"), "phải nói là có khoá: {in_ra}");

    // Including inside a whole list, since `{:?}` on a `Vec` calls each element's `Debug`.
    let danh_sach = format!("{:?}", store.list().expect("liệt kê"));
    assert!(!danh_sach.contains("sk-khong-duoc-in-ra"));
}

#[test]
fn xoa_cai_dang_hoat_dong_thi_cai_khac_len_thay() {
    let store = store();
    let mot = them(&store, "Một", None);
    let hai = them(&store, "Hai", None);

    let dang_chay = store
        .activate(Role::Chat, mot.id(), Some("mo-hinh"))
        .expect("ghim");
    assert!(dang_chay.active_chat);
    assert_eq!(dang_chay.model.as_deref(), Some("mo-hinh"));

    store.remove(mot.id()).expect("xoá");

    let con_lai = store
        .active(Role::Chat)
        .expect("đọc")
        .expect("phải còn một cái");
    assert_eq!(con_lai.id(), hai.id());
    assert!(
        store
            .list()
            .expect("liệt kê")
            .iter()
            .all(|row| row.id() != mot.id())
    );

    // Deleting the last one leaves nobody active, which is a valid answer rather than a dead id.
    store.remove(hai.id()).expect("xoá nốt");
    assert!(store.active(Role::Chat).expect("đọc").is_none());
}

#[cfg(unix)]
#[test]
fn tep_co_so_du_lieu_chi_chu_no_doc_duoc() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join(pai_providers::DB_FILE);
    let store = SqliteProviderStore::open(&path).expect("mở kho");
    them(&store, "OpenAI", Some("bi-mat"));

    let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "quyền thực tế: {:o}", mode & 0o777);
}

#[cfg(unix)]
#[test]
fn tep_qua_ho_thi_bi_siet_lai_luc_mo() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("thư mục tạm");
    let path = dir.path().join(pai_providers::DB_FILE);
    std::fs::write(&path, b"").expect("tạo tệp");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("nới quyền");

    let _store = SqliteProviderStore::open(&path).expect("mở kho");
    let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
