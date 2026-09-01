//! Ngân sách token và vòng lấy lại phần tràn.
//!
//! Bài quan trọng nhất ở đây là bài cuối: nó đi đúng con đường mô hình đi — đọc mã vé
//! **từ chính văn bản kết quả**, rồi gọi `spill_read` bằng mã đó. Một lời nhắn "toàn văn
//! vẫn còn" mà không ai đi lại được đường đó thì chỉ là một câu để tin rồi đi tiếp.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolPipeline, ToolRegistry, ToolSchema,
    Tools, ToolsPlugin, approx_tokens,
};
use serde_json::json;

/// Một tool nhả ra rất nhiều dòng đánh số — đủ để nhận ra dòng nào bị bỏ.
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

/// Khoá: **byte chia bốn, không phải số dòng.** Hai chuỗi cùng số dòng mà khác kích thước
/// phải cho hai con số khác nhau; đó chính là chỗ trần theo dòng đo nhầm.
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

/// Khoá: **vòng lấy lại khép kín.** Đường ống gấp kết quả, nói ra mã vé trong nội dung,
/// và `spill_read` gọi bằng đúng mã đó trả về phần giữa đã bị bỏ.
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

    // Đọc mã vé ra khỏi **văn bản mô hình đọc**, không phải khỏi `meta` — `meta` không đi
    // ra tới mô hình, nên một mã chỉ nằm ở đó là một mã mô hình không gõ lại được.
    let marker = "`spill_read` với `id: \"";
    let start = cut.content.find(marker).expect("nội dung phải in ra mã vé") + marker.len();
    let id = &cut.content[start..start + cut.content[start..].find('"').expect("mã có dấu đóng")];

    let back = pipeline
        // `limit` nhỏ vì chính `spill_read` cũng chịu ngân sách — đọc lại nguyên khối
        // chỉ chuyển chỗ tràn chứ không giải quyết gì.
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

/// Khoá: **vé không còn giá trị là một câu trả lời đọc được, không phải một lỗi câm.**
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
