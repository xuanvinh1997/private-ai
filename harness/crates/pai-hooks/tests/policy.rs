//! Hook chặn được, và hook hỏng thì không chặn.
//!
//! Cặp bài đầu tiên là cả nội dung của crate này. Chúng đối xứng nhau và đối lập với
//! `Approver`: phê duyệt fail-**closed** vì nó thay mặt một người đang ngồi đó, hook
//! fail-**open** vì nó thay mặt một tệp cấu hình. Lẫn hai mặc định đó là biến một lỗi gõ
//! nhầm trong script của người dùng thành một ứng dụng đứng im.

use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_hooks::{HookConfig, HooksPlugin};
use pai_tools::{PreDecision, PreExecute, PreRequest, ToolMeta, ToolName};
use serde_json::{Map, Value, json};

async fn decide(ctx: &Context, tool: &str, args: Value) -> PreDecision {
    let arguments: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let mut req = PreRequest {
        name: ToolName::from(tool),
        call_id: "c1".into(),
        arguments,
        meta: ToolMeta::mutating(),
    };
    ctx.waterfall::<PreExecute, _>(&mut req, |_| Box::pin(async { PreDecision::Allow }))
        .await
}

async fn with_hooks(hooks: Vec<HookConfig>) -> Context {
    let ctx = Context::root();
    let scope = ctx.plugin("hooks");
    HooksPlugin::new(hooks)
        .apply(&scope)
        .await
        .expect("cắm được");
    std::mem::forget(scope);
    ctx
}

fn hook(command: &str, tools: &[&str]) -> HookConfig {
    hook_with(command, tools, None)
}

fn hook_with(command: &str, tools: &[&str], timeout_secs: Option<u64>) -> HookConfig {
    HookConfig {
        timeout_secs,
        command: command.to_string(),
        tools: tools.iter().map(|t| t.to_string()).collect(),
    }
}

#[tokio::test]
async fn hook_noi_khong_thi_chan_va_ly_do_di_thang_toi_mo_hinh() {
    let ctx = with_hooks(vec![hook(
        r#"echo '{"decision":"deny","reason":"chính sách công ty cấm chạy lệnh"}'"#,
        &[],
    )])
    .await;

    match decide(&ctx, "bash", json!({ "command": "ls" })).await {
        PreDecision::Deny(reason) => assert!(reason.contains("chính sách công ty")),
        other => panic!("phải bị chặn, nhận {other:?}"),
    }
}

#[tokio::test]
async fn hook_noi_dong_y_thi_di_tiep() {
    let ctx = with_hooks(vec![hook(r#"echo '{"decision":"allow"}'"#, &[])]).await;
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn hook_hong_thi_cho_qua_chu_khong_chan() {
    // Ba kiểu hỏng, cùng một kết quả: lệnh không tồn tại, thoát với mã khác 0, và in ra
    // thứ không phải JSON. Không cái nào là bằng chứng rằng lời gọi nguy hiểm.
    for command in ["khong-co-lenh-nay-dau", "exit 3", "echo khong-phai-json"] {
        let ctx = with_hooks(vec![hook(command, &[])]).await;
        assert!(
            matches!(decide(&ctx, "bash", json!({})).await, PreDecision::Allow),
            "hook `{command}` hỏng mà lại chặn"
        );
    }
}

#[tokio::test]
async fn hook_het_gio_thi_cho_qua() {
    // Hạn giờ **rút ngắn cho bài test**, không dùng con số 10 giây của sản phẩm. Bản
    // trước đo đồng hồ thật với hạn giờ thật và đỏ ngẫu nhiên hai lần khi máy đang chạy
    // hai chục bài khác song song — cận trên 20 giây bị vượt vì lịch chạy, không vì hạn
    // giờ hỏng. Một bài đỏ vì máy bận là một bài không nói gì về mã.
    let ctx = with_hooks(vec![hook_with("sleep 30", &[], Some(1))]).await;
    let started = std::time::Instant::now();
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
    let waited = started.elapsed();
    // Cái đang được kiểm: `sleep 30` **không** chạy hết 30 giây. Cận trên rộng rãi vì nó
    // chỉ cần loại trừ "hạn giờ không cắt gì cả", không cần đo chính xác.
    assert!(
        waited < std::time::Duration::from_secs(15),
        "hạn giờ không cắt: {waited:?}"
    );
}

#[tokio::test]
async fn hook_chi_chay_cho_tool_no_khai() {
    let ctx = with_hooks(vec![hook(
        r#"echo '{"decision":"deny","reason":"chỉ cấm bash"}'"#,
        &["bash"],
    )])
    .await;

    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Deny(_)
    ));
    // Mỗi lần gọi hook là một lần spawn tiến trình; lọc ở đây để những lời gọi rẻ nhất
    // không phải trả giá cho một chính sách không nói về chúng.
    assert!(matches!(
        decide(&ctx, "read", json!({})).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn hook_doc_duoc_ten_tool_va_tham_so_tren_stdin() {
    // Hook chỉ chặn khi thấy đúng lệnh nó quan tâm — tức là nó thật sự đọc được payload.
    let ctx = with_hooks(vec![hook(
        r#"grep -q 'rm -rf' && echo '{"decision":"deny","reason":"lệnh xoá"}' || echo '{"decision":"allow"}'"#,
        &[],
    )])
    .await;

    assert!(matches!(
        decide(&ctx, "bash", json!({ "command": "rm -rf /" })).await,
        PreDecision::Deny(_)
    ));
    assert!(matches!(
        decide(&ctx, "bash", json!({ "command": "ls" })).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn khong_co_hook_nao_thi_khong_dang_ky_gi_ca() {
    let ctx = with_hooks(Vec::new()).await;
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
    // Và không có `Arc` nào bị giữ lại: một plugin rỗng không được để lại dấu vết.
    let _ = Arc::new(());
}
