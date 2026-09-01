//! Danh tính dự án, và cây tệp.

use std::path::Path;

use pai_project::{ProjectKind, ProjectStore, SqliteProjectStore, list_tree, read_file};
use rusqlite::Connection;
use tempfile::TempDir;

fn store() -> SqliteProjectStore {
    SqliteProjectStore::open_in_memory().expect("mở kho")
}

#[test]
fn hai_loi_vao_cung_mot_thu_muc_la_mot_du_an() {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải");
    std::fs::create_dir_all(root.join("con")).expect("tạo thư mục con");
    let store = store();

    let direct = store.touch(&root).expect("mở được");
    // Cùng thư mục, tới bằng một lối khác. Không chuẩn hoá thì đây thành hàng thứ hai, và
    // người dùng có hai mục trỏ cùng một chỗ, mỗi mục nhớ một nửa lịch sử.
    let round_about = store.touch(&root.join("con").join("..")).expect("mở được");

    assert_eq!(direct.id, round_about.id);
    assert_eq!(store.list().expect("liệt kê").len(), 1);
}

#[test]
fn mo_lai_thi_len_dau_danh_sach() {
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let store = store();
    let first = store.touch(a.path()).expect("mở được");
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.touch(b.path()).expect("mở được");
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.touch(a.path()).expect("mở lại");

    // Mới nhất trước — thứ tự người ta nghĩ tới khi mở lại một dự án.
    assert_eq!(store.list().expect("liệt kê")[0].id, first.id);
}

#[test]
fn bo_khoi_danh_sach_khong_dung_toi_dia() {
    let dir = TempDir::new().expect("thư mục tạm");
    let marker = dir.path().join("con-nguyen.txt");
    std::fs::write(&marker, "còn").expect("ghi tệp");
    let store = store();
    let project = store.touch(dir.path()).expect("mở được");

    store.forget(&project.id).expect("bỏ được");
    assert!(store.list().expect("liệt kê").is_empty());
    // Nhầm chỗ này là mất việc của người ta.
    assert!(marker.exists(), "bỏ khỏi danh sách mà lại xoá thư mục");
    assert!(store.forget(&project.id).is_err(), "bỏ hai lần phải nói ra");
}

#[test]
fn duong_khong_phai_thu_muc_bi_tu_choi() {
    let dir = TempDir::new().expect("thư mục tạm");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "x").expect("ghi tệp");
    assert!(store().touch(&file).is_err());
    assert!(store().touch(&dir.path().join("khong-co")).is_err());
}

#[test]
fn cay_ton_trong_gitignore_va_sap_thu_muc_truoc() {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải");
    std::fs::write(root.join(".gitignore"), "target/\n").expect("ghi");
    std::fs::create_dir_all(root.join("target")).expect("tạo");
    std::fs::create_dir_all(root.join("src")).expect("tạo");
    std::fs::write(root.join("a.rs"), "fn main() {}").expect("ghi");

    let names: Vec<String> = list_tree(&root, None, 1)
        .expect("đọc được")
        .into_iter()
        .map(|e| e.name)
        .collect();

    assert!(
        !names.contains(&"target".to_string()),
        "`target/` lọt qua .gitignore: {names:?}"
    );
    // Thư mục trước rồi tới tệp — thứ tự mọi trình duyệt tệp dùng.
    assert_eq!(names.first().map(String::as_str), Some("src"));
    assert!(names.contains(&"a.rs".to_string()));
}

#[test]
fn cay_mac_dinh_chi_mot_cap() {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải");
    std::fs::create_dir_all(root.join("src/sau")).expect("tạo");

    let one = list_tree(&root, None, 1).expect("đọc được");
    // `None` nghĩa là "chưa nạp", khác hẳn `Some(vec![])` nghĩa là "thư mục trống".
    assert!(one[0].children.is_none());
    let two = list_tree(&root, None, 2).expect("đọc được");
    assert!(two[0].children.is_some());
}

#[test]
fn khong_doc_duoc_tep_ngoai_du_an() {
    let dir = TempDir::new().expect("thư mục tạm");
    let outside = TempDir::new().expect("thư mục tạm");
    let secret = outside.path().join("bi-mat.txt");
    std::fs::write(&secret, "không được").expect("ghi");

    assert!(read_file(dir.path(), &secret).is_err());
    assert!(read_file(dir.path(), Path::new("/etc/hosts")).is_err());
}

#[test]
fn tep_qua_dai_bi_cat_va_noi_ra() {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải");
    let file = root.join("dai.txt");
    let content: String = (0..25_000).map(|n| format!("dòng {n}\n")).collect();
    std::fs::write(&file, &content).expect("ghi");

    let view = read_file(&root, &file).expect("đọc được");
    assert!(view.truncated);
    // Số dòng thật vẫn báo đủ: cắt để không dựng một triệu phần tử DOM, không phải để
    // nói dối về kích thước tệp.
    assert_eq!(view.total_lines, 25_000);
    assert!(view.text.lines().count() <= 20_000);
    assert_eq!(view.lang.as_deref(), Some("txt"));
}

/// Cơ sở dữ liệu của người dùng đang chạy không có hai cột mới. Nó phải sống sót.
///
/// Danh sách dự án là thứ người ta tự gõ vào từng dòng; dựng lại nó thì không có nguồn
/// nào để dựng, và mở ứng dụng lên thấy danh sách trống là mất việc của người ta.
#[test]
fn schema_cu_duoc_them_cot_tai_cho_va_khong_mat_hang_nao() {
    let conn = Connection::open_in_memory().expect("mở kết nối");
    conn.execute_batch(
        "CREATE TABLE projects (
           id             TEXT    PRIMARY KEY,
           path           TEXT    NOT NULL UNIQUE,
           name           TEXT    NOT NULL,
           last_opened_at INTEGER NOT NULL
         ) STRICT;
         INSERT INTO projects VALUES ('mot', '/nha/mot', 'mot', 10);
         INSERT INTO projects VALUES ('hai', '/nha/hai', 'hai', 20);",
    )
    .expect("dựng schema cũ");

    let store = SqliteProjectStore::from_connection(conn).expect("mở kho trên db cũ");
    let danh_sach = store.list().expect("liệt kê");

    assert_eq!(danh_sach.len(), 2, "migrate mà mất hàng");
    for du_an in &danh_sach {
        assert_eq!(
            du_an.kind,
            ProjectKind::Code,
            "hàng cũ phải là dự án mã nguồn"
        );
        assert_eq!(du_an.origin, None, "hàng cũ không từ đâu clone về");
    }
    assert_eq!(danh_sach[0].id, "hai", "mới nhất vẫn phải lên đầu");
}

/// Mở lại một dự án tài liệu **không** được biến nó thành dự án mã nguồn.
///
/// Một `ON CONFLICT DO UPDATE` viết ẩu làm đúng việc đó, im lặng, và chỉ lộ ra khi tool
/// chạy lệnh bỗng xuất hiện trong một thư mục toàn tệp người ngoài gửi tới.
#[test]
fn touch_giu_nguyen_loai_cua_hang_da_co() {
    let dir = TempDir::new().expect("thư mục tạm");
    let store = store();
    let tao = store
        .create(
            dir.path(),
            ProjectKind::Docs,
            Some("https://vi.du/tai-lieu.git"),
        )
        .expect("tạo được");

    let mo_lai = store.touch(dir.path()).expect("mở lại");

    assert_eq!(mo_lai.id, tao.id, "vẫn phải là một dự án");
    assert_eq!(mo_lai.kind, ProjectKind::Docs, "mở lại mà đổi mất loại");
    assert_eq!(
        mo_lai.origin.as_deref(),
        Some("https://vi.du/tai-lieu.git"),
        "mở lại mà quên mất chỗ nó từ đâu tới"
    );
    assert!(mo_lai.last_opened_at >= tao.last_opened_at);
}

#[test]
fn create_va_list_tra_dung_loai_va_nguon() {
    let (ma, tai_lieu) = (TempDir::new().expect("tạm"), TempDir::new().expect("tạm"));
    let store = store();
    store
        .create(ma.path(), ProjectKind::Code, None)
        .expect("tạo");
    store
        .create(
            tai_lieu.path(),
            ProjectKind::Docs,
            Some("https://vi.du/x.git"),
        )
        .expect("tạo");

    let danh_sach = store.list().expect("liệt kê");
    let tim = |kind| {
        danh_sach
            .iter()
            .find(|du_an| du_an.kind == kind)
            .expect("phải có")
    };
    assert_eq!(tim(ProjectKind::Code).origin, None);
    assert_eq!(
        tim(ProjectKind::Docs).origin.as_deref(),
        Some("https://vi.du/x.git")
    );
    assert_eq!(danh_sach.len(), 2);
}

#[test]
fn set_kind_doi_duoc_loai_va_noi_ra_khi_khong_co_du_an() {
    let dir = TempDir::new().expect("thư mục tạm");
    let store = store();
    let du_an = store.touch(dir.path()).expect("mở được");
    assert_eq!(du_an.kind, ProjectKind::Code);

    let doi = store
        .set_kind(&du_an.id, ProjectKind::Docs)
        .expect("đổi được");
    assert_eq!(doi.kind, ProjectKind::Docs);
    assert_eq!(
        store.get(&du_an.id).expect("đọc lại").kind,
        ProjectKind::Docs
    );
    assert!(store.set_kind("khong-co", ProjectKind::Docs).is_err());
}
