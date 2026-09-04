//! Token budgets and the spill round trip.
//! The last test walks the model's own path: read the ticket id out of the result text, then
//! call `spill_read` with it, since a promise nobody can follow is only a promise.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolPipeline, ToolRegistry, ToolSchema,
    Tools, ToolsPlugin, approx_tokens,
};
use serde_json::json;

/// A tool emitting many numbered lines, enough to tell which ones were dropped.
struct Numbered;

#[async_trait]
impl Tool for Numbered {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "fake.numbered",
            "nhả nhiều dòng",
            json!({ "type": "object", "properties": {} }),
        )
    }
    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }
    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::ok(
            (1..=3000)
                .map(|n| format!("dòng {n}\n"))
                .collect::<String>(),
        ))
    }
}

/// Locks bytes-over-four rather than line counts: two strings of equal line count but different size must differ.
#[test]
fn ngan_sach_dem_byte_chu_khong_dem_dong() {
    let thua: String = (0..100).map(|_| "x\n".to_string()).collect();
    let day: String = (0..100).map(|_| format!("{}\n", "x".repeat(200))).collect();
    assert_eq!(thua.lines().count(), day.lines().count());
    assert!(
        approx_tokens(&day) > approx_tokens(&thua) * 50,
        "cùng 100 dòng mà ngân sách phải nhìn thấy chênh lệch"
    );
}

/// Locks the closed round trip: the pipeline folds, names the ticket in the content, and `spill_read` returns the middle.
#[tokio::test]
async fn ma_ve_in_ra_trong_noi_dung_goi_lai_duoc_bang_spill_read() {
    let ctx = Context::root();
    ToolsPlugin
        .apply(&ctx.plugin("tools"))
        .await
        .expect("cắm được tools");
    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().expect("có sổ đăng ký");
    ctx.keep(registry.register(Arc::new(Numbered)));

    let pipeline = ToolPipeline::new(&ctx, registry).with_token_budget(200);
    let cut = pipeline.execute("c1", "fake__numbered", json!({})).await;

    assert!(cut.content.contains("dòng 1\n"), "{}", cut.content);
    assert!(cut.content.contains("dòng 3000"), "{}", cut.content);
    assert!(
        !cut.content.contains("dòng 1500\n"),
        "phần giữa lẽ ra phải bị gấp đi"
    );

    // Read the id from the text the model reads, not from `meta`, which never reaches the model.
    let marker = "`spill_read` với `id: \"";
    let start = cut.content.find(marker).expect("nội dung phải in ra mã vé") + marker.len();
    let id = &cut.content[start..start + cut.content[start..].find('"').expect("mã có dấu đóng")];

    let back = pipeline
        // A small `limit`, since `spill_read` is budgeted too and reading it whole just moves the overflow.
        .execute(
            "c2",
            "spill_read",
            json!({ "id": id, "offset": 1495, "limit": 20 }),
        )
        .await;
    assert!(!back.is_error, "{}", back.content);
    assert!(
        back.content.contains("dòng 1500"),
        "phần giữa phải lấy lại được:\n{}",
        &back.content[..300.min(back.content.len())]
    );
}

/// Locks that an expired ticket is a readable answer, not a silent error.
#[tokio::test]
async fn ve_khong_ton_tai_thi_noi_ro_phai_lam_gi() {
    let ctx = Context::root();
    ToolsPlugin
        .apply(&ctx.plugin("tools"))
        .await
        .expect("cắm được tools");
    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().expect("có sổ đăng ký");

    let outcome = ToolPipeline::new(&ctx, registry)
        .execute("c1", "spill_read", json!({ "id": "spill-khong-co" }))
        .await;
    assert!(outcome.is_error);
    assert!(
        outcome.content.contains("chạy lại tool đã sinh ra nó"),
        "{}",
        outcome.content
    );
}
