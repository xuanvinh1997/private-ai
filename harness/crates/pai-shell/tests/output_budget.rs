//! Output lệnh cũng là output lớn.
//!
//! Một `cargo build` hay một bộ test đỏ nhả ra vài trăm KiB, và phần đuôi — mã thoát,
//! dòng lỗi cuối — là phần đáng giá nhất. Cắt chỉ-lấy-đầu ở đây dạy mô hình rằng một
//! lệnh chạy mười nghìn dòng không có kết cục nào cả.

use std::path::PathBuf;
use std::sync::Arc;

use pai_core::Context;
use pai_sandbox::Policy;
use pai_shell::jobs::Jobs;
use pai_shell::provider::{LocalShell, ShellExecutor};
use pai_shell::tools::bash::Bash;
use pai_tools::{
    Invocation, MemorySpillStore, Overflow, Spill, SpillRef, SpillStore, Tool, ToolName,
};
use serde_json::{Map, Value, json};

fn call(args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from("bash"), "c1", map)
}

/// Khoá: **output dài bị gấp lại, không bị cắt cụt** — cả đầu lẫn đuôi còn, toàn văn nằm
/// trong kho, và kết quả nói ra cách lấy tiếp.
#[tokio::test]
async fn output_bash_rat_dai_bi_gap_lai_va_do_vao_kho() {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("cắm được kho tràn"),
    );

    let shell: Arc<dyn ShellExecutor> = Arc::new(LocalShell::new(
        ctx.clone(),
        Policy::danger_full_access("/tmp"),
    ));
    let bash = Bash::new(
        shell,
        Arc::new(Jobs::default()),
        PathBuf::from("/tmp"),
        Overflow::new(&ctx).with_budget(200),
    );

    let outcome = bash
        .execute(&call(json!({
            "command": "seq 1 5000 | sed 's/^/dong /'; echo XONG-CUOI"
        })))
        .await
        .expect("chạy được");

    assert!(!outcome.is_error, "{}", outcome.content);
    assert!(
        outcome.content.contains("dong 1\n"),
        "mất phần đầu:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("XONG-CUOI"),
        "mất phần đuôi — đây mới là chỗ có kết cục của lệnh:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "cắt trong im lặng:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("`spill_read` với `id:"),
        "không nói cách lấy toàn văn:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("| tail -n 200"),
        "không nói cách lọc ngay trong lệnh:\n{}",
        outcome.content
    );

    let handle: SpillRef = serde_json::from_value(
        outcome
            .meta
            .get("spill")
            .cloned()
            .expect("kết quả bị cắt phải mang vé"),
    )
    .expect("vé đọc được");
    let full = store.read(&handle).expect("vé còn giá trị");
    assert!(full.contains("dong 2500"), "phần giữa không được mất");
    assert!(
        outcome.content.len() < full.len() / 4,
        "phần gửi cho mô hình vẫn dài: {} byte",
        outcome.content.len()
    );
}

/// Khoá: **output vừa ngân sách thì đi nguyên vẹn** — không có vé, không có lời cắt nào.
/// Không có bài này thì "luôn cắt" cũng làm bài trên xanh.
#[tokio::test]
async fn output_ngan_di_nguyen_ven_va_khong_sinh_ve() {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("cắm được kho tràn"),
    );

    let shell: Arc<dyn ShellExecutor> = Arc::new(LocalShell::new(
        ctx.clone(),
        Policy::danger_full_access("/tmp"),
    ));
    let bash = Bash::new(
        shell,
        Arc::new(Jobs::default()),
        PathBuf::from("/tmp"),
        Overflow::new(&ctx).with_budget(200),
    );

    let outcome = bash
        .execute(&call(json!({ "command": "echo xin-chao" })))
        .await
        .expect("chạy được");

    assert!(outcome.content.contains("xin-chao"));
    assert!(
        !outcome.content.contains("đã cắt bớt"),
        "{}",
        outcome.content
    );
    assert!(outcome.meta.get("spill").is_none());
    assert!(store.is_empty(), "không cắt thì không cất gì");
}
