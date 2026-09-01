//! Bộ chọn phạm vi tool có thật sự siết được lõi hay không.
//!
//! Mọi bài ở đây đi qua **đúng hai lời gọi mà vòng lặp agent gọi** — `registry.schemas`
//! cho danh sách gửi cho mô hình, và `ToolPipeline::execute` cho lần gọi thật — trên
//! chính ngữ cảnh mà `run_turn` dựng. Kiểm một tầng thôi thì không đủ: một mô hình đọc
//! lại bản ghi phiên vẫn nhớ tên tool của lượt trước và gọi thẳng, không đi qua danh
//! sách bao giờ.

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

/// Người duyệt luôn đồng ý.
///
/// Có mặt vì `bash` đi qua `PreDecision::Ask`, và không có ai để hỏi thì nó bị từ chối
/// **vì lý do khác** — bài test sẽ xanh mà chẳng chứng minh được gì về phạm vi. Ở đây ta
/// muốn đúng một biến số: phạm vi.
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

/// Danh sách tên tool mà mô hình nhìn thấy trong một phạm vi — tầng lọc thứ nhất.
fn mo_hinh_thay(registry: &ToolRegistry, ctx: &Context) -> Vec<String> {
    registry
        .schemas(ctx.scope_key())
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect()
}

/// Gọi thẳng một tool qua đường ống — tầng lọc thứ hai.
async fn goi_thang(
    harness: &Harness,
    ctx: &Context,
    name: &str,
    args: serde_json::Value,
) -> pai_tools::ToolOutcome {
    let pipeline = ToolPipeline::new(ctx, so_dang_ky(harness));
    pipeline.execute("goi-thu", name, args).await
}

/// Lời gọi có bị chặn bởi **chính sách phạm vi** không.
///
/// Phân biệt với mọi kiểu hỏng khác bằng `meta.refusal`: một lệnh `bash` hỏng vì không
/// tìm thấy chương trình cũng là `is_error`, và gộp hai thứ đó lại là viết một bài test
/// xanh vì lý do sai.
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
    // Có người duyệt và người đó đồng ý: thứ duy nhất còn chặn được lệnh này là phạm vi.

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
    // Và nó bị chặn **trước** thân tool: một lời từ chối sau khi lệnh đã chạy là một lời
    // từ chối vô nghĩa.
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

    // `edit` đòi đã đọc tệp trước — một chính sách khác, không phải phạm vi. Đi qua nó
    // để bài test đo đúng một biến số.
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

    // `edit` đòi đã đọc tệp trước — một chính sách khác, không phải phạm vi. Đi qua nó
    // để bài test đo đúng một biến số.
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

    // Lượt song song của một phiên khác, mở trong khi lượt trên còn sống: hai lượt không
    // được siết lẫn nhau, nếu không thì một câu hỏi chỉ đọc ở tab này sẽ âm thầm cắt tool
    // của lượt đang chạy ở tab kia.
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

    // Và lượt sau bắt đầu lại từ đầu, không thừa hưởng gì.
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

    // Phạm vi gốc chưa bao giờ bị chạm tới: hạn chế gắn vào phạm vi của lượt, nên không
    // có gì rò rỉ ra ngoài nó.
    let goc: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .collect();
    assert!(goc.contains(&"bash".to_string()), "phạm vi gốc mất bash");
}

/// Mở một lượt là **cắm luôn người duyệt**.
///
/// Bài này khoá lại một lỗi đã nằm trong sản phẩm: `app/src/approval.rs` có đủ hai nửa —
/// hỏi và trả lời — nhưng không ai cắm nó vào seam `Approval`, và đường ống tool thì
/// fail-closed. Nên `bash` chưa từng chạy được một lần nào: mô hình thấy nó trong danh
/// sách, gọi nó, và bị từ chối vì *không có ai để hỏi*. Triệu chứng giống hệt "mô hình
/// không biết gọi tool", nên nó sống sót qua nhiều lượt kiểm mắt.
#[tokio::test]
async fn mo_mot_luot_la_cam_luon_nguoi_duyet() {
    let dir = TempDir::new().expect("thư mục tạm");
    let harness = boot(config(&dir)).await.expect("dựng được cây");

    // Gốc **không** có người duyệt, và đúng như vậy: câu hỏi duyệt đi ra bằng `Channel`
    // của một lượt cụ thể, nên không có lượt thì không có ai để hỏi.
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

    // Và dọn lượt là gỡ luôn — không có người duyệt nào sống sót sang lượt sau.
    luot.effects().dispose().await;
    assert!(harness.ctx.get::<pai_tools::Approval>().is_none());

    harness.shutdown().await;
}
