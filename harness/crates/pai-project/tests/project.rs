//! Danh tính dự án, và cây tệp.

use std::path::Path;

use pai_project::{ProjectStore, SqliteProjectStore, list_tree, read_file};
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
