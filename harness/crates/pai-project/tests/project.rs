//! Danh tính dự án, và cây tệp.

use pai_project::{ProjectKind, ProjectStore, SqliteProjectStore};
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

/// Thêm lại bằng tay một thư mục đã clone về không được xoá mất chỗ nó từ đâu tới.
///
/// Đường thêm bằng tay chỉ có đường dẫn trong tay, nó **không biết** URL. Ghi thẳng
/// `origin = excluded.origin` ở đây là dùng cái nó không biết để đè lên cái đã biết.
#[test]
fn them_lai_bang_tay_khong_xoa_mat_cho_no_tu_dau_toi() {
    let dir = TempDir::new().expect("thư mục tạm");
    let store = store();
    store
        .create(dir.path(), ProjectKind::Code, Some("https://vi.du/x.git"))
        .expect("clone về");

    let lai = store
        .create(dir.path(), ProjectKind::Code, None)
        .expect("thêm lại bằng tay");

    assert_eq!(lai.origin.as_deref(), Some("https://vi.du/x.git"));
}

/// Ngược với `touch`: ở `create` người dùng vừa nói ra loại, nên loại mới thắng.
///
/// Hai đường phải ngược nhau đúng ở điểm này. Cho `create` giữ nguyên loại cũ nghĩa là
/// người dùng chọn "tài liệu" trong hộp thoại rồi nhận về một dự án mã nguồn, không có
/// thông báo nào.
#[test]
fn create_noi_ro_loai_thi_loai_moi_thang() {
    let dir = TempDir::new().expect("thư mục tạm");
    let store = store();
    let dau = store
        .create(dir.path(), ProjectKind::Docs, None)
        .expect("tạo");
    let sau = store
        .create(dir.path(), ProjectKind::Code, None)
        .expect("khai lại loại");

    assert_eq!(sau.id, dau.id, "vẫn phải là một dự án");
    assert_eq!(sau.kind, ProjectKind::Code);
}

/// `set_kind` là đường ra duy nhất khỏi một dự án vào nhầm loại.
///
/// Loại đặt một lần lúc ghi nhận và `touch` cố ý giữ nguyên nó, nên thiếu đường này thì
/// một repo mã nguồn lỡ ghi nhận thành thư viện tài liệu sẽ mãi mãi không có `read`,
/// `grep` hay `bash` — và người dùng chỉ thấy trợ lý nói nó không có tool nào.
#[test]
fn set_kind_la_duong_ra_khoi_ngo_cut() {
    let dir = TempDir::new().expect("thư mục tạm");
    let store = store();
    let nham = store
        .create(dir.path(), ProjectKind::Docs, None)
        .expect("tạo");

    let sua = store
        .set_kind(&nham.id, ProjectKind::Code)
        .expect("đổi được loại");
    assert_eq!(sua.kind, ProjectKind::Code);
    assert_eq!(
        store.get(&nham.id).expect("đọc lại").kind,
        ProjectKind::Code
    );

    // Và mở lại sau đó vẫn giữ loại đã sửa.
    assert_eq!(
        store.touch(dir.path()).expect("mở lại").kind,
        ProjectKind::Code
    );
}

/// Một `id` không có thật thì mọi đường đều nói ra, và nói ra **cái id ấy**.
///
/// Một lỗi sqlite thô ("query returned no rows") đi ngược lên tới giao diện thì người
/// dùng đọc được một câu không dính gì tới việc họ vừa làm. Gọi tên cái id là thứ duy
/// nhất phân biệt "dự án này không còn nữa" với "kho hỏng".
#[test]
fn id_khong_co_that_thi_moi_duong_deu_goi_ten_no_ra() {
    let store = store();
    for loi in [
        store.get("khong-co").expect_err("phải là lỗi").to_string(),
        store
            .forget("khong-co")
            .expect_err("phải là lỗi")
            .to_string(),
        store
            .set_kind("khong-co", ProjectKind::Docs)
            .expect_err("phải là lỗi")
            .to_string(),
    ] {
        assert!(loi.contains("khong-co"), "lỗi không gọi tên id: {loi}");
    }
}

/// Một loại lạ trong cơ sở dữ liệu đọc chệch về `code`, chứ không làm mất cả hàng.
///
/// Xảy ra khi người dùng chạy một bản mới hơn — bản ấy ghi xuống một loại thứ ba — rồi mở
/// lại bản cũ. Mất một cái nhãn thì sửa được bằng một cú bấm; từ chối cả hàng thì mất một
/// dự án khỏi danh sách, mà danh sách này không dựng lại được từ đâu.
#[test]
fn loai_la_trong_co_so_du_lieu_doc_chech_ve_ma_nguon() {
    let conn = Connection::open_in_memory().expect("mở kết nối");
    conn.execute_batch(
        "CREATE TABLE projects (
           id             TEXT    PRIMARY KEY,
           path           TEXT    NOT NULL UNIQUE,
           name           TEXT    NOT NULL,
           last_opened_at INTEGER NOT NULL,
           kind           TEXT    NOT NULL DEFAULT 'code',
           origin         TEXT
         ) STRICT;
         INSERT INTO projects VALUES ('la', '/nha/la', 'la', 10, 'ban-do', NULL);",
    )
    .expect("dựng hàng có loại lạ");

    let store = SqliteProjectStore::from_connection(conn).expect("mở kho");
    let danh_sach = store
        .list()
        .expect("một loại lạ không được làm hỏng cả lời gọi");

    assert_eq!(
        danh_sach.len(),
        1,
        "mất hàng vì một cái nhãn không đọc được"
    );
    assert_eq!(danh_sach[0].kind, ProjectKind::Code);
}
