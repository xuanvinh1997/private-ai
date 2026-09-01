//! Ngân sách, phần tràn, và câu hỏi "thư mục này có gì".
//!
//! Mỗi bài ở đây khoá một chỗ mù đã có thật: kết quả bị cắt trong im lặng, trần đếm nhầm
//! đơn vị, một kho lớn làm `grep` chạy mãi, và một kho lạ mà không tool nào trả lời được
//! câu hỏi đầu tiên của mô hình.

use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_fs::path::FileRoots;
use pai_fs::provider::{FsProvider, LocalFs};
use pai_fs::tools::{grep::Grep, list::ListDir, read::Read};
use pai_fs::{FsPlugin, ReadLedger};
use pai_tools::Spill;
use pai_tools::{
    Invocation, MemorySpillStore, Overflow, Resolution, SpillRef, SpillStore, Tool, ToolName,
    ToolPipeline, ToolRegistry, Tools, ToolsPlugin,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

/// Một cây có kho tràn cắm sẵn — không có nó thì không có gì bị cắt, đúng theo thiết kế.
fn tree() -> (Context, Arc<MemorySpillStore>) {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("cắm được kho tràn"),
    );
    (ctx, store)
}

fn bench() -> (TempDir, FileRoots) {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải gốc");
    let roots = FileRoots::new([root.clone()], [root.join("bi-mat")]);
    (dir, roots)
}

fn spill_of(outcome: &pai_tools::ToolOutcome) -> SpillRef {
    serde_json::from_value(
        outcome
            .meta
            .get("spill")
            .cloned()
            .expect("kết quả bị cắt phải mang vé lấy lại"),
    )
    .expect("vé đọc được")
}

// --- 1. đọc một tệp vượt ngân sách ------------------------------------------------------

/// Khoá: **cắt là gấp lại, không phải vứt đi.** Kết quả phải có cả đầu lẫn đuôi, phải
/// mang vé, và phải nói ra *bằng chữ mô hình đọc được* cách lấy phần còn lại.
///
/// Khẳng định đi thẳng vào chuỗi thật chứ không vào "có trường nào đó": một trường
/// `truncated = true` mà nội dung không nói gì thì mô hình vẫn kết luận nó đã thấy hết.
#[tokio::test]
async fn doc_tep_vuot_ngan_sach_thi_co_ca_dau_lan_duoi_va_chi_dan_doc_tiep() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let file = dir.path().canonicalize().unwrap().join("dai.txt");
    let content: String = (1..=4000).map(|n| format!("dòng số {n}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let read = Read::new(
        Arc::new(LocalFs) as Arc<dyn FsProvider>,
        roots,
        Arc::new(ReadLedger::default()),
        Overflow::new(&ctx).with_budget(200),
    );
    let outcome = read
        .execute(&call(
            "read",
            // `limit` tường minh: ngân sách là một trần **độc lập**, không phải một cách
            // viết khác của `limit`. Xin đủ 4000 dòng rồi vẫn bị gấp lại mới chứng minh
            // được điều đó.
            json!({ "file_path": file.display().to_string(), "limit": 4000 }),
        ))
        .await
        .expect("đọc được");

    // Đầu.
    assert!(
        outcome.content.contains("dòng số 1\n"),
        "mất phần đầu:\n{}",
        outcome.content
    );
    // Đuôi. Không có nó, mô hình không biết tệp kết thúc ở đâu.
    assert!(
        outcome.content.contains("dòng số 4000"),
        "mất phần đuôi:\n{}",
        outcome.content
    );
    // Chỉ dẫn lấy tiếp, nói ra bằng chữ và **cụ thể**.
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "không nói là đã cắt:\n{}",
        outcome.content
    );
    assert!(
        outcome
            .content
            .contains("`read` với `file_path` như cũ và `offset:"),
        "không nói cách đọc tiếp:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("`spill_read` với `id:"),
        "không nói cách lấy toàn văn:\n{}",
        outcome.content
    );

    // Toàn văn còn nguyên.
    let handle = spill_of(&outcome);
    let full = store.read(&handle).expect("vé còn giá trị");
    assert!(
        full.contains("dòng số 2000"),
        "phần giữa phải còn trong kho"
    );
    assert!(
        outcome.content.len() < full.len() / 2,
        "phần gửi cho mô hình vẫn dài"
    );
}

// --- 2. đếm dòng là đếm nhầm thứ --------------------------------------------------------

/// Khoá: **ngân sách đo byte, không đo dòng.** Năm dòng thì mọi trần theo dòng đều cho
/// qua, nhưng năm dòng này nặng 15 KiB.
#[tokio::test]
async fn it_dong_ma_dong_rat_dai_van_bi_cat_theo_ngan_sach() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let file = dir.path().canonicalize().unwrap().join("mot-dong.json");
    let content: String = (0..5)
        .map(|n| format!("{}{}\n", (b'a' + n) as char, "x".repeat(3000)))
        .collect();
    std::fs::write(&file, &content).unwrap();

    let read = Read::new(
        Arc::new(LocalFs) as Arc<dyn FsProvider>,
        roots,
        Arc::new(ReadLedger::default()),
        Overflow::new(&ctx).with_budget(200),
    );
    let outcome = read
        .execute(&call(
            "read",
            json!({ "file_path": file.display().to_string() }),
        ))
        .await
        .expect("đọc được");

    // Chỉ năm dòng — một trần "256 dòng" hay `limit: 2000` sẽ thả trọn cả tệp đi qua.
    let read_meta = outcome.meta.get("read").expect("có meta read");
    assert_eq!(read_meta["total_lines"], json!(5));
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "ít dòng mà dòng dài vẫn phải bị cắt:\n{}",
        &outcome.content[..200.min(outcome.content.len())]
    );

    let handle = spill_of(&outcome);
    assert!(
        store.read(&handle).map(|s| s.len()).unwrap_or(0) > 15_000,
        "toàn văn phải còn nguyên trong kho"
    );
    assert!(
        outcome.content.len() < 2_000,
        "kết quả gửi đi phải nằm quanh ngân sách 200 token, đang là {} byte",
        outcome.content.len()
    );
}

// --- 3. grep trên kho lớn ---------------------------------------------------------------

/// Khoá: **chạm trần thì nói ra.** Một danh sách cụt trông y hệt một danh sách đầy đủ.
#[tokio::test]
async fn grep_cham_tran_so_khop_thi_noi_ra_va_do_spill() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let root = dir.path().canonicalize().unwrap();
    // Nhiều khớp hơn trần, trong một tệp — đúng hình dạng của một tệp sinh mã.
    let content: String = (0..6_000).map(|n| format!("khop {n}\n")).collect();
    std::fs::write(root.join("nhieu.txt"), content).unwrap();

    let outcome = Grep::new(roots, Overflow::new(&ctx))
        .execute(&call("grep", json!({ "pattern": "khop" })))
        .await
        .expect("tìm được");

    assert!(
        outcome.content.contains("đã dừng ở 5000 khớp"),
        "không nói là đã chạm trần:\n{}",
        &outcome.content[outcome.content.len().saturating_sub(600)..]
    );
    assert!(
        outcome
            .content
            .contains("thu hẹp bằng `path` hoặc `include`")
            || outcome
                .content
                .contains("Hãy thu hẹp bằng `path` hoặc `include`"),
        "không nói cách thu hẹp:\n{}",
        &outcome.content[outcome.content.len().saturating_sub(600)..]
    );

    let search = outcome.meta.get("search").expect("có meta search");
    assert_eq!(search["total"], json!(5000), "trần chặn ở chỗ thu thập");
    assert_eq!(search["truncated"], json!(true));

    let handle = spill_of(&outcome);
    let full = store.read(&handle).expect("toàn văn còn trong kho");
    assert!(full.contains("khop 2500"), "phần giữa không được mất");
    assert!(outcome.content.len() < full.len() / 4);
}

// --- 4. thư mục này có gì ---------------------------------------------------------------

/// Khoá bốn thứ cùng lúc: đường dẫn được bảo vệ **bị giấu khỏi danh sách** (luật 3),
/// `.gitignore` có hiệu lực **ngoài kho git** (`require_git(false)`), tệp ẩn vẫn hiện, và
/// thứ tự là thư mục trước rồi theo tên.
#[tokio::test]
async fn list_dir_giau_duong_dan_duoc_bao_ve_va_ton_trong_gitignore() {
    let (ctx, _) = tree();
    let (dir, roots) = bench();
    let root = dir.path().canonicalize().unwrap();

    std::fs::write(root.join("bi-mat"), "mã thông báo").unwrap();
    std::fs::write(root.join(".gitignore"), "bo-qua/\n").unwrap();
    std::fs::create_dir_all(root.join("bo-qua")).unwrap();
    std::fs::write(root.join("bo-qua/rac.txt"), "rác").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("zeta.txt"), "z").unwrap();
    std::fs::write(root.join("alpha.txt"), "a".repeat(2048)).unwrap();

    let outcome = ListDir::new(roots, Overflow::new(&ctx))
        .execute(&call("list_dir", json!({})))
        .await
        .expect("liệt kê được");
    let text = &outcome.content;

    assert!(
        !text.contains("bi-mat"),
        "danh sách lộ tệp được bảo vệ:\n{text}"
    );
    // Thư mục tạm này **không** phải kho git. Không có `require_git(false)` thì
    // `.gitignore` bị bỏ qua và `bo-qua` hiện ra.
    assert!(
        !text.contains("bo-qua"),
        "`.gitignore` không có hiệu lực ngoài kho git:\n{text}"
    );
    assert!(
        text.contains(".gitignore"),
        "tệp ẩn phải hiện — nó nói dự án chạy bằng cách nào:\n{text}"
    );
    assert!(text.contains("src/"), "thư mục phải có dấu `/`:\n{text}");
    assert!(text.contains("2.0 KB"), "phải kèm kích thước:\n{text}");

    let dir_at = text.find("src/").expect("có src");
    let file_at = text.find("alpha.txt").expect("có alpha.txt");
    assert!(dir_at < file_at, "thư mục phải đứng trước tệp:\n{text}");
    assert!(
        text.find("alpha.txt") < text.find("zeta.txt"),
        "tệp phải theo tên:\n{text}"
    );
}

// --- 6. tool mới đăng ký được vào sổ thật ------------------------------------------------

/// Khoá: **`list_dir` là một tool thật trong cây thật**, không phải một struct chỉ gọi
/// được từ bài test. Đi qua đúng đường mà mô hình đi: sổ đăng ký, tên dạng wire, đường ống.
#[tokio::test]
async fn list_dir_dang_ky_duoc_vao_so_that_va_goi_duoc() {
    let dir = TempDir::new().expect("thư mục tạm");
    let root = dir.path().canonicalize().expect("phân giải gốc");
    std::fs::write(root.join("co-that.txt"), "nội dung").unwrap();

    let ctx = Context::root();
    ToolsPlugin
        .apply(&ctx.plugin("tools"))
        .await
        .expect("cắm được tools");
    FsPlugin::new([root.clone()], [root.join("bi-mat")])
        .apply(&ctx.plugin("fs"))
        .await
        .expect("cắm được fs");

    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().expect("có sổ đăng ký");
    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|s| s.name.as_str().to_string())
        .collect();
    assert!(names.contains(&"list_dir".to_string()), "{names:?}");
    assert!(
        names.contains(&"spill_read".to_string()),
        "không có `spill_read` thì lời nhắn \"toàn văn vẫn còn\" là lời hứa suông: {names:?}"
    );

    // Tra bằng đúng cái tên mô hình gõ.
    assert!(matches!(
        registry.resolve(None, "list_dir"),
        Resolution::Found(_, _)
    ));

    let outcome = ToolPipeline::new(&ctx, registry)
        .execute("c1", "list_dir", json!({}))
        .await;
    assert!(!outcome.is_error, "{}", outcome.content);
    assert!(
        outcome.content.contains("co-that.txt"),
        "{}",
        outcome.content
    );
}
