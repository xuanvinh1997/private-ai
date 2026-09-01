//! Những bất biến mà một coding agent sai thì mất tệp của người dùng.
//!
//! Mỗi bài ở đây khoá một câu đã viết trong tài liệu. Nếu một bài đỏ thì hoặc mã sai,
//! hoặc câu trong tài liệu đã hết đúng — không có khả năng thứ ba.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pai_core::Context;
use pai_fs::path::FileRoots;
use pai_fs::provider::{FsProvider, LocalFs};
use pai_fs::tools::{edit::Edit, glob::GlobTool, grep::Grep, read::Read, write::Write};
use pai_fs::{ReadLedger, looks_binary};
use pai_tools::{Invocation, Overflow, Tool, ToolName, ToolOutcome};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

/// Ngân sách không có kho tràn nào phía sau, nên nó không cắt gì.
///
/// Cố ý: những bài trong tệp này kiểm chính sách đường dẫn và hình dạng kết quả, và một
/// lần cắt xen vào giữa sẽ khiến chúng đỏ vì một lý do không liên quan. Việc cắt được
/// kiểm riêng ở `budget.rs`.
fn no_budget() -> Overflow {
    Overflow::new(&Context::root())
}

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

fn bench() -> (TempDir, FileRoots, Arc<dyn FsProvider>, Arc<ReadLedger>) {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải gốc");
    let roots = FileRoots::new([root.clone()], [root.join("bi-mat")]);
    (
        dir,
        roots,
        Arc::new(LocalFs),
        Arc::new(ReadLedger::default()),
    )
}

async fn read_ok(read: &Read, path: &Path) -> ToolOutcome {
    read.execute(&call(
        "read",
        json!({ "file_path": path.display().to_string() }),
    ))
    .await
    .expect("đọc được")
}

#[tokio::test]
async fn dau_cham_cham_va_symlink_khong_thoat_khoi_goc() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("ngoai.txt");
    std::fs::write(&secret, "không được đọc").unwrap();

    let read = Read::new(fs, roots, ledger, no_budget());

    // Đi lên bằng `..`.
    let escape = root.join("..").join(secret.file_name().unwrap());
    let err = read
        .execute(&call(
            "read",
            json!({ "file_path": escape.display().to_string() }),
        ))
        .await;
    assert!(err.is_err(), "`..` không được thoát khỏi gốc");

    // Đi ra bằng symlink nằm *trong* gốc.
    #[cfg(unix)]
    {
        let link = root.join("loi-tat.txt");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let err = read
            .execute(&call(
                "read",
                json!({ "file_path": link.display().to_string() }),
            ))
            .await;
        assert!(err.is_err(), "symlink trỏ ra ngoài phải bị coi là ở ngoài");
    }
}

#[tokio::test]
async fn duong_dan_duoc_bao_ve_khong_doc_duoc_va_khong_hien_trong_listing() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let secret = root.join("bi-mat");
    std::fs::write(&secret, "mã thông báo").unwrap();
    std::fs::write(root.join("thuong.txt"), "bình thường").unwrap();

    let read = Read::new(fs, roots.clone(), ledger, no_budget());
    let err = read
        .execute(&call(
            "read",
            json!({ "file_path": secret.display().to_string() }),
        ))
        .await
        .expect_err("tệp được bảo vệ không đọc được");
    assert!(
        err.to_string().contains("được bảo vệ"),
        "lý do phải là lý do đúng: {err}"
    );

    // Và nó cũng không được lộ ra qua đường liệt kê — chặn đọc mà vẫn kể tên là đã nói
    // cho mô hình biết có cái gì ở đó để mà đi tìm đường khác.
    let listing = GlobTool::new(roots)
        .execute(&call("glob", json!({ "pattern": "*" })))
        .await
        .expect("liệt kê được");
    assert!(
        !listing.content.contains("bi-mat"),
        "listing lộ tệp được bảo vệ:\n{}",
        listing.content
    );
    assert!(listing.content.contains("thuong.txt"));
}

#[tokio::test]
async fn tep_nhi_phan_bi_tu_choi_thay_vi_tra_ve_rac() {
    assert!(looks_binary(&[0x7f, b'E', b'L', b'F', 0x00]));
    assert!(!looks_binary("chào bạn".as_bytes()));

    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let binary = root.join("a.bin");
    std::fs::write(&binary, [0x00, 0x01, 0x02]).unwrap();

    let err = Read::new(fs, roots, ledger, no_budget())
        .execute(&call(
            "read",
            json!({ "file_path": binary.display().to_string() }),
        ))
        .await
        .expect_err("tệp nhị phân bị từ chối");
    assert!(err.to_string().contains("nhị phân"), "{err}");
}

#[tokio::test]
async fn edit_khop_nhieu_lan_thi_loi_va_khong_sua_gi() {
    let (dir, roots, fs, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("a.rs");
    let before = "let x = 1;\nlet x = 1;\n";
    std::fs::write(&file, before).unwrap();

    let edit = Edit::new(fs, roots);
    let err = edit
        .execute(&call(
            "edit",
            json!({
                "file_path": file.display().to_string(),
                "old_string": "let x = 1;",
                "new_string": "let x = 2;",
            }),
        ))
        .await
        .expect_err("khớp hai chỗ thì phải từ chối");
    assert!(
        err.to_string().contains('2'),
        "lỗi phải nói khớp bao nhiêu chỗ: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        before,
        "tệp phải y nguyên"
    );

    // Nói rõ ý định thì làm được.
    edit.execute(&call(
        "edit",
        json!({
            "file_path": file.display().to_string(),
            "old_string": "let x = 1;",
            "new_string": "let x = 2;",
            "replace_all": true,
        }),
    ))
    .await
    .expect("replace_all sửa được");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "let x = 2;\nlet x = 2;\n"
    );
}

#[tokio::test]
async fn so_dong_trong_hunk_la_so_dong_that_trong_tep() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("a.txt");
    // Thay đổi nằm ở dòng 12, đủ xa để phân biệt "đếm trong hunk" với "đếm trong tệp".
    let before: String = (1..=20).map(|n| format!("dòng {n}\n")).collect();
    std::fs::write(&file, &before).unwrap();

    let read = Read::new(fs.clone(), roots.clone(), ledger, no_budget());
    read_ok(&read, &file).await;

    let outcome = Edit::new(fs, roots)
        .execute(&call(
            "edit",
            json!({
                "file_path": file.display().to_string(),
                "old_string": "dòng 12",
                "new_string": "dòng mười hai",
            }),
        ))
        .await
        .expect("sửa được");

    let diffs = outcome
        .meta
        .get("diffs")
        .and_then(|v| v.as_array())
        .expect("có diffs");
    let hunk = diffs.first().expect("một hunk");
    // Ngữ cảnh ba dòng, nên hunk bắt đầu ở dòng 9.
    assert_eq!(
        hunk["old_start"],
        json!(9),
        "hunk phải mang vị trí thật: {hunk}"
    );
    assert_eq!(hunk["new_start"], json!(9));
}

#[tokio::test]
async fn write_tep_moi_thi_old_text_la_null_chu_khong_phai_chuoi_rong() {
    let (dir, roots, fs, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("moi.txt");

    let outcome = Write::new(fs, roots)
        .execute(&call(
            "write",
            json!({ "file_path": file.display().to_string(), "content": "xin chào\n" }),
        ))
        .await
        .expect("tạo được");

    let diffs = outcome
        .meta
        .get("diffs")
        .and_then(|v| v.as_array())
        .expect("có diffs");
    // `null` nghĩa là "tệp mới"; chuỗi rỗng nghĩa là "tệp cũ vốn rỗng". Giao diện vẽ hai
    // thứ đó khác nhau, nên chúng không được phép lẫn.
    assert_eq!(diffs[0]["old_text"], Value::Null);
}

#[tokio::test]
async fn grep_bo_qua_tep_nhi_phan_va_dem_du_tong_so() {
    let (dir, roots, _, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.txt"), "cần tìm\nkhông\ncần tìm\n").unwrap();
    let mut binary = vec![0u8];
    binary.extend_from_slice("cần tìm".as_bytes());
    binary.push(0);
    std::fs::write(root.join("b.bin"), binary).unwrap();

    let outcome = Grep::new(roots, no_budget())
        .execute(&call("grep", json!({ "pattern": "cần tìm" })))
        .await
        .expect("tìm được");

    let search = outcome.meta.get("search").expect("có search");
    assert_eq!(
        search["total"],
        json!(2),
        "tệp nhị phân không được góp khớp"
    );
    assert_eq!(search["shape"], json!("matches"));
    assert!(!outcome.content.contains("b.bin"));
}

#[tokio::test]
async fn glob_khong_liet_ke_thu_muc_va_mau_khong_co_gach_cheo_khop_moi_do_sau() {
    let (dir, roots, _, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src/sau")).unwrap();
    std::fs::write(root.join("src/a.rs"), "").unwrap();
    std::fs::write(root.join("src/sau/b.rs"), "").unwrap();

    let outcome = GlobTool::new(roots)
        .execute(&call("glob", json!({ "pattern": "*.rs" })))
        .await
        .expect("tìm được");

    // Không có luật "khớp tên tệp ở mọi độ sâu" thì kết quả này rỗng, và mô hình kết
    // luận sai rằng repo không có tệp Rust nào.
    assert!(outcome.content.contains("a.rs"));
    assert!(
        outcome.content.contains("b.rs"),
        "mẫu không có `/` phải khớp mọi độ sâu"
    );
    assert!(
        !outcome.content.contains("src\n"),
        "thư mục không được liệt kê"
    );
}

#[test]
fn goc_trong_nghia_la_tu_choi_tat_chu_khong_phai_cho_phep_tat() {
    let roots = FileRoots::new(Vec::<PathBuf>::new(), Vec::<PathBuf>::new());
    assert!(roots.resolve_read(Path::new("/etc/hosts")).is_err());
}
