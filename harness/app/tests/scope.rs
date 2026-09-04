//! Whether the tool-scope picker actually restricts the core. Every test goes through the two calls the agent
//! loop makes -- `registry.schemas` and `ToolPipeline::execute` -- on the context `run_turn` builds, because a
//! model replaying a session remembers earlier tool names and calls them without consulting the list.

use std::sync::Arc;

use async_trait::async_trait;
use pai_app_lib::harness::{Config, Harness, boot};
use pai_app_lib::protocol::ToolScope;
use pai_app_lib::scope::mo_pham_vi;
use pai_core::Context;
use pai_tools::{ApprovalRequest, Approver, ToolPipeline, ToolRegistry, Tools};
use serde_json::json;
use tempfile::TempDir;

fn config(dir: &TempDir) -> Config {
    Config {
        data_dir: dir.path().join("du-lieu"),
        builtin_skills: None,
        embed_model: None,
        workspace: Some(dir.path().to_path_buf()),
        ollama_url: "http://127.0.0.1:11434".into(),
        model: "mo-hinh-thu".into(),
        context_window: 32_768,
    }
}

/// An approver that always says yes, because `bash` goes through `PreDecision::Ask` and with nobody to ask it
/// would be denied for the wrong reason, leaving one variable: the scope.
struct LuonDongY;

#[async_trait]
impl Approver for LuonDongY {
    async fn approve(&self, _request: &ApprovalRequest) -> bool {
        true
    }
}

fn so_dang_ky(harness: &Harness) -> Arc<ToolRegistry> {
    harness.ctx.require::<Tools>().expect("có sổ đăng ký")
}

/// The tool names the model sees in a scope -- the first filter layer.
fn mo_hinh_thay(registry: &ToolRegistry, ctx: &Context) -> Vec<String> {
    registry
        .schemas(ctx.scope_key())
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect()
}

/// Call a tool directly through the pipeline -- the second filter layer.
async fn goi_thang(
    harness: &Harness,
    ctx: &Context,
    name: &str,
    args: serde_json::Value,
) -> pai_tools::ToolOutcome {
    let pipeline = ToolPipeline::new(ctx, so_dang_ky(harness));
    pipeline.execute("goi-thu", name, args).await
}

/// Whether a call was blocked by scope policy, told apart from other failures by `meta.refusal`: a `bash`
/// command failing on a missing binary is also `is_error`.
fn bi_pham_vi_chan(outcome: &pai_tools::ToolOutcome) -> bool {
    outcome.meta.get("refusal") == Some(&json!("denied"))
}

#[tokio::test]
async fn pham_vi_chi_doc_thi_mo_hinh_khong_thay_bash_va_edit() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = so_dang_ky(&harness);

    let luot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Read,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    let names = mo_hinh_thay(&registry, &luot);

    for cam in ["bash", "edit", "write", "task"] {
        assert!(
            !names.contains(&cam.to_string()),
            "phạm vi chỉ đọc vẫn quảng cáo `{cam}`: {names:?}"
        );
    }
    for con in ["read", "grep", "glob"] {
        assert!(
            names.contains(&con.to_string()),
            "phạm vi chỉ đọc mất `{con}`: {names:?}"
        );
    }

    luot.effects().dispose().await;
}

#[tokio::test]
async fn pham_vi_chi_doc_thi_goi_thang_bash_van_bi_tu_choi() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    // An approver exists and agrees, so the only thing left that can block this is the scope.

    let dau = dir.path().join("dau-vet.txt");
    let luot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Read,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    let outcome = goi_thang(
        &harness,
        &luot,
        "bash",
        json!({ "command": format!("echo co-chay > {}", dau.display()) }),
    )
    .await;

    assert!(bi_pham_vi_chan(&outcome), "bash không bị chặn: {outcome:?}");
    // And it is blocked before the tool body: a refusal after the command ran is meaningless.
    assert!(!dau.exists(), "lệnh đã chạy dù bị từ chối");

    luot.effects().dispose().await;
}

#[tokio::test]
async fn pham_vi_doc_ghi_thi_edit_chay_duoc_con_bash_bi_chan() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = so_dang_ky(&harness);

    let tep = dir.path().join("ghi-chu.txt");
    std::fs::write(&tep, "cu\n").expect("dựng tệp thử");

    let luot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Write,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    let names = mo_hinh_thay(&registry, &luot);
    assert!(names.contains(&"edit".to_string()), "mất `edit`: {names:?}");
    assert!(
        !names.contains(&"bash".to_string()),
        "còn `bash`: {names:?}"
    );

    // `edit` requires a prior read -- a different policy, not scope -- so satisfy it and keep one variable.
    let doc = goi_thang(
        &harness,
        &luot,
        "read",
        json!({ "file_path": tep.display().to_string() }),
    )
    .await;
    assert!(!doc.is_error, "read không chạy được: {doc:?}");

    let sua = goi_thang(
        &harness,
        &luot,
        "edit",
        json!({
            "file_path": tep.display().to_string(),
            "old_string": "cu",
            "new_string": "moi",
        }),
    )
    .await;
    assert!(!sua.is_error, "edit không chạy được: {sua:?}");
    assert_eq!(
        std::fs::read_to_string(&tep).expect("đọc lại"),
        "moi\n",
        "edit chạy nhưng tệp không đổi"
    );

    let chay = goi_thang(
        &harness,
        &luot,
        "bash",
        json!({ "command": "echo xin-chao" }),
    )
    .await;
    assert!(bi_pham_vi_chan(&chay), "bash không bị chặn: {chay:?}");

    luot.effects().dispose().await;
}

#[tokio::test]
async fn pham_vi_chay_lenh_thi_ca_edit_lan_bash_deu_chay() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    let tep = dir.path().join("ghi-chu.txt");
    std::fs::write(&tep, "cu\n").expect("dựng tệp thử");

    let luot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Shell,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");

    // `edit` requires a prior read -- a different policy, not scope -- so satisfy it and keep one variable.
    let doc = goi_thang(
        &harness,
        &luot,
        "read",
        json!({ "file_path": tep.display().to_string() }),
    )
    .await;
    assert!(!doc.is_error, "read không chạy được: {doc:?}");

    let sua = goi_thang(
        &harness,
        &luot,
        "edit",
        json!({
            "file_path": tep.display().to_string(),
            "old_string": "cu",
            "new_string": "moi",
        }),
    )
    .await;
    assert!(!sua.is_error, "edit không chạy được: {sua:?}");

    let chay = goi_thang(
        &harness,
        &luot,
        "bash",
        json!({ "command": "echo xin-chao" }),
    )
    .await;
    assert!(!bi_pham_vi_chan(&chay), "bash bị phạm vi chặn: {chay:?}");
    assert!(
        chay.content.contains("xin-chao"),
        "bash không chạy thật: {chay:?}"
    );

    luot.effects().dispose().await;
}

#[tokio::test]
async fn pham_vi_la_cua_mot_luot_khong_dinh_sang_luot_sau() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");
    let registry = so_dang_ky(&harness);

    let mot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Read,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    assert!(
        !mo_hinh_thay(&registry, &mot).contains(&"bash".to_string()),
        "lượt chỉ đọc vẫn thấy bash"
    );

    // A concurrent turn from another session, opened while the one above is alive: neither may restrict the other.
    let song_song = mo_pham_vi(
        &harness.ctx,
        ToolScope::Shell,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    assert!(
        mo_hinh_thay(&registry, &song_song).contains(&"bash".to_string()),
        "lượt chạy-lệnh bị lượt chỉ-đọc siết theo"
    );
    song_song.effects().dispose().await;
    mot.effects().dispose().await;

    // And the next turn starts fresh, inheriting nothing.
    let hai = mo_pham_vi(
        &harness.ctx,
        ToolScope::Shell,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    let names = mo_hinh_thay(&registry, &hai);
    for tro_lai in ["bash", "edit", "write"] {
        assert!(
            names.contains(&tro_lai.to_string()),
            "lượt sau vẫn mất `{tro_lai}`: {names:?}"
        );
    }
    let chay = goi_thang(
        &harness,
        &hai,
        "bash",
        json!({ "command": "echo lai-duoc" }),
    )
    .await;
    assert!(!bi_pham_vi_chan(&chay), "lượt sau vẫn bị chặn: {chay:?}");
    hai.effects().dispose().await;

    // The root scope is never touched: the restriction binds to the turn's scope, so nothing leaks out of it.
    let goc: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect();
    assert!(goc.contains(&"bash".to_string()), "phạm vi gốc mất bash");
}

/// Opening a turn installs the approver, locking a shipped bug: `approval.rs` had both halves but was never
/// provided to the `Approval` seam, and with a fail-closed pipeline `bash` was always denied for lack of anyone to ask.
#[tokio::test]
async fn mo_mot_luot_la_cam_luon_nguoi_duyet() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // The root has no approver, correctly: prompts leave through a specific turn's `Channel`.
    assert!(
        harness.ctx.get::<pai_tools::Approval>().is_none(),
        "gốc không nên có người duyệt"
    );

    let luot = mo_pham_vi(
        &harness.ctx,
        ToolScope::Shell,
        Arc::new(LuonDongY) as Arc<dyn Approver>,
    )
    .expect("mở được phạm vi");
    assert!(
        luot.get::<pai_tools::Approval>().is_some(),
        "mở lượt mà không cắm người duyệt: mọi lời xin duyệt sẽ bị từ chối"
    );

    // And disposing the turn removes it: no approver survives into the next turn.
    luot.effects().dispose().await;
    assert!(harness.ctx.get::<pai_tools::Approval>().is_none());

    harness.shutdown().await;
}
