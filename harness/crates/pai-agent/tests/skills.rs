//! Progressive disclosure, and skill selection.
//! The three-tier test matters most: if full instructions reach the prompt before selection,
//! a hundred skills cost a hundred documents instead of a hundred lines.

use std::sync::Arc;

use pai_agent::{
    PreStep, PreStepRequest, Prompt, SkillRegistry, SkillsPlugin, StepDecision, SystemPrompt,
};
use pai_core::{Context, Plugin};
use pai_session::Message;
use tempfile::TempDir;

fn write_skill(root: &TempDir, name: &str, front: &str, body: &str) {
    let dir = root.path().join(name);
    std::fs::create_dir_all(&dir).expect("tạo thư mục");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\n{front}\n---\n\n{body}\n"),
    )
    .expect("ghi SKILL.md");
}

fn registry(root: &TempDir) -> Arc<SkillRegistry> {
    let registry = SkillRegistry::new();
    registry.scan(root.path());
    registry
}

#[test]
fn ba_tang_tiet_lo_dan() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "tra-cuu-hop-dong",
        "name: tra-cuu-hop-dong\ntitle: Tra cứu hợp đồng\ndescription: Tìm điều khoản trong hợp đồng.\nkeywords: [hợp đồng, điều khoản]",
        "## Quy trình\n1. Tìm điều khoản bằng đúng cụm từ người dùng nêu.",
    );
    std::fs::write(root.path().join("tra-cuu-hop-dong/mau.md"), "mẫu").expect("ghi tệp phụ");
    let registry = registry(&root);

    // Tier one: always name and description, never the body.
    let catalog = registry.catalog().expect("có danh mục");
    assert!(catalog.contains("tra-cuu-hop-dong"));
    assert!(catalog.contains("Tìm điều khoản"));
    // Assert on a sentence only the body has: the catalogue heading shares its wording.
    assert!(
        !catalog.contains("đúng cụm từ người dùng nêu"),
        "thân hướng dẫn lọt vào tầng một:\n{catalog}"
    );
    assert!(
        !catalog.contains("mau.md"),
        "tên tệp phụ chỉ xuất hiện ở tầng hai"
    );

    // Before selection, tier two is empty.
    assert!(registry.activated().is_none());
}

#[test]
fn chon_xong_thi_toan_van_moi_vao_prompt() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "tra-cuu-hop-dong",
        "name: tra-cuu-hop-dong\ntitle: Tra cứu hợp đồng\ndescription: Tìm điều khoản.\nkeywords: [hợp đồng]",
        "## Quy trình\n1. Luôn dẫn nguồn theo số điều.",
    );
    std::fs::write(root.path().join("tra-cuu-hop-dong/mau.md"), "mẫu").expect("ghi tệp phụ");
    let registry = registry(&root);

    let chosen = registry.select("cho tôi xem điều khoản trong hợp đồng này");
    assert_eq!(chosen, vec!["tra-cuu-hop-dong".to_string()]);
}

#[test]
fn go_dau_roi_moi_so_khop() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "tom-tat-tai-lieu",
        "name: tom-tat-tai-lieu\ntitle: Tóm tắt tài liệu\ndescription: Tóm tắt một tài liệu dài.\nkeywords: [tài liệu, tóm tắt]",
        "## Quy trình\n1. Đọc rồi tóm.",
    );
    let registry = registry(&root);

    // Typing without diacritics still has to match, or selection almost never fires.
    assert_eq!(registry.select("tom tat tai lieu giup toi").len(), 1);
    assert_eq!(registry.select("tóm tắt tài liệu giúp tôi").len(), 1);
    assert!(registry.select("hôm nay trời đẹp").is_empty());
}

#[test]
fn goi_thieu_mo_ta_hoac_thieu_than_thi_bi_bo_qua_chu_khong_lam_hong_lan_quet() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "thieu-mo-ta",
        "name: thieu-mo-ta",
        "có thân nhưng không có mô tả",
    );
    write_skill(
        &root,
        "thieu-than",
        "name: thieu-than\ndescription: có mô tả.",
        "",
    );
    write_skill(
        &root,
        "lanh-lan",
        "name: lanh-lan\ndescription: Gói lành lặn.",
        "## Quy trình\n1. Xong.",
    );
    let registry = registry(&root);

    assert_eq!(registry.len(), 1, "chỉ gói lành lặn được nhận");
    assert!(
        registry
            .catalog()
            .expect("có danh mục")
            .contains("lanh-lan")
    );
}

#[tokio::test]
async fn plugin_dong_gop_dung_hai_khoi_va_go_ra_thi_sach() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "viet-test",
        "name: viet-test\ndescription: Viết bài kiểm chứng.",
        "## Quy trình\n1. Viết bài đỏ trước.",
    );

    let ctx = Context::root();
    let prompt = SystemPrompt::new();
    ctx.provide::<Prompt>(prompt.clone())
        .expect("cắm được")
        .leak();

    let scope = ctx.plugin("skills");
    SkillsPlugin::new([root.path().to_path_buf()])
        .apply(&scope)
        .await
        .expect("cắm được");

    assert!(prompt.assemble().contains("viet-test"));
    scope.effects().dispose().await;
    assert!(
        !prompt.assemble().contains("viet-test"),
        "gỡ plugin phải thu hồi cả hai khối prompt"
    );
}

/* ── Chọn skill: hai cái van, và cả hai đều tồn tại để prompt không phình ra ──── */

/// One common word must not drag the whole register into a turn; without the relative floor, long questions match everything.
#[test]
fn san_tuong_doi_gat_bo_cai_chi_trung_mot_tu() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "chinh",
        "name: doi-chieu-hop-dong\ntitle: Đối chiếu hợp đồng\ndescription: So hai bản hợp đồng.\nkeywords: [hợp đồng, đối chiếu]",
        "## Quy trình\n1. So từng điều.",
    );
    // Only one keyword hits: past the absolute threshold, but far below the leader.
    write_skill(
        &root,
        "phu",
        "name: luu-tru\ntitle: Lưu trữ\ndescription: Cất bản cuối vào kho.\nkeywords: [hợp đồng]",
        "## Quy trình\n1. Cất đi.",
    );
    let registry = registry(&root);

    let chosen = registry.select("đối chiếu hợp đồng giúp tôi");
    assert_eq!(
        chosen,
        vec!["doi-chieu-hop-dong".to_string()],
        "sàn tương đối phải gạt cái chỉ chung một từ khoá: {chosen:?}"
    );
}

/// A word in the description is not an invocation: prose words score half a point, not two.
#[test]
fn mot_tu_trong_mo_ta_chua_du_de_chon() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "ve-bieu-do",
        "name: ve-bieu-do\ntitle: Vẽ biểu đồ\ndescription: Chuyển bảng tính thành biểu đồ.\nkeywords: [biểu đồ]",
        "## Quy trình\n1. Vẽ.",
    );
    let registry = registry(&root);

    // "thanh" is a long word in the description, and nothing more.
    assert!(
        registry.select("giúp tôi làm thành phẩm").is_empty(),
        "một từ trong mô tả mà đủ điểm thì mọi câu hỏi dài đều kéo theo skill này"
    );
    // One keyword, on the other hand, is enough.
    assert_eq!(registry.select("vẽ biểu đồ").len(), 1);
}

/// A later package replaces one of the same name rather than joining it, which is how user packages override built-ins.
#[test]
fn goi_quet_sau_thay_the_goi_trung_ten() {
    let (dung_san, cua_toi) = (
        TempDir::new().expect("thư mục tạm"),
        TempDir::new().expect("thư mục tạm"),
    );
    write_skill(
        &dung_san,
        "tom-tat",
        "name: tom-tat\ndescription: Bản dựng sẵn.",
        "## Quy trình\n1. Bản dựng sẵn.",
    );
    write_skill(
        &cua_toi,
        "tom-tat",
        "name: tom-tat\ndescription: Bản của tôi.",
        "## Quy trình\n1. Bản của tôi.",
    );

    let registry = SkillRegistry::new();
    registry.scan(dung_san.path());
    registry.scan(cua_toi.path());

    assert_eq!(registry.len(), 1, "trùng tên phải là thay thế");
    let catalog = registry.catalog().expect("có danh mục");
    assert!(catalog.contains("Bản của tôi."));
    assert!(!catalog.contains("Bản dựng sẵn."));
}

/// The name goes into the prompt and a directory name, so it is narrow, and a bad one is skipped, not repaired.
#[test]
fn ten_khong_hop_le_thi_goi_bi_bo_qua() {
    let root = TempDir::new().expect("thư mục tạm");
    for (dir, name) in [
        ("hoa", "Tom-Tat"),
        ("khoang-trang", "tom tat"),
        ("gach-cheo", "../thoat-ra"),
        ("co-dau", "tóm-tắt"),
        ("rong", ""),
    ] {
        write_skill(
            &root,
            dir,
            &format!("name: \"{name}\"\ndescription: Có mô tả."),
            "## Quy trình\n1. Xong.",
        );
    }
    write_skill(
        &root,
        "hop-le",
        "name: hop-le\ndescription: Có mô tả.",
        "## Quy trình\n1. Xong.",
    );

    let registry = registry(&root);
    assert_eq!(registry.len(), 1, "chỉ tên hợp lệ được nhận");
}

/* ── Tầng hai và tầng ba, đi qua đúng con đường thật ──────────────────────────── */

/// Mount the plugin on a `Context` that outlives this call.
async fn cam_skills(root: &TempDir) -> (Context, Arc<pai_agent::SystemPrompt>) {
    let ctx = Context::root();
    let prompt = SystemPrompt::new();
    ctx.provide::<Prompt>(prompt.clone())
        .expect("cắm được")
        .leak();
    let scope = ctx.plugin("skills");
    SkillsPlugin::new([root.path().to_path_buf()])
        .apply(&scope)
        .await
        .expect("cắm được");
    std::mem::forget(scope);
    (ctx, prompt)
}

async fn chay_pre_step(ctx: &Context, messages: Vec<Message>) {
    let mut req = PreStepRequest {
        turn: 1,
        step: 1,
        messages,
        history: Vec::new(),
    };
    ctx.waterfall::<PreStep, _>(&mut req, |req| {
        let messages = req.messages.clone();
        Box::pin(async move { StepDecision::enter(messages) })
    })
    .await;
}

/// Full instructions enter the prompt only after selection, and sibling files only by name; this locks the other direction.
#[tokio::test]
async fn chon_roi_thi_toan_van_va_ten_tep_moi_vao_prompt() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "ve-so-do",
        "name: ve-so-do\ntitle: Vẽ sơ đồ\ndescription: Dựng sơ đồ khối.\nkeywords: [sơ đồ]",
        "## Quy trình\n1. Chỉ dùng cú pháp mermaid.",
    );
    std::fs::write(root.path().join("ve-so-do/mau.mmd"), "graph TD; a-->b;").expect("ghi tệp phụ");
    std::fs::write(root.path().join("ve-so-do/.ghi-chu"), "riêng tư").expect("ghi tệp ẩn");

    let (ctx, prompt) = cam_skills(&root).await;
    assert!(
        !prompt.assemble().contains("cú pháp mermaid"),
        "chưa có lượt nào mà thân đã vào prompt"
    );

    chay_pre_step(&ctx, vec![Message::user("vẽ sơ đồ cho tôi")]).await;

    let day_du = prompt.assemble();
    assert!(
        day_du.contains("cú pháp mermaid"),
        "chọn rồi mà toàn văn không vào prompt:\n{day_du}"
    );
    assert!(
        day_du.contains("mau.mmd"),
        "tầng ba phải nêu tên tệp đi kèm, nếu không mô hình không biết có gì để mở"
    );
    assert!(
        !day_du.contains("graph TD;"),
        "tầng ba chỉ nêu **tên** tệp — nhét nội dung vào là bỏ luôn tầng ba"
    );
    assert!(
        !day_du.contains(".ghi-chu"),
        "tệp ẩn không phải tài nguyên của skill"
    );
}

/// Later steps carry no new message, and the procedure must stay: clearing it loses the rules mid-turn.
#[tokio::test]
async fn buoc_khong_co_van_ban_moi_thi_giu_nguyen_quy_trinh() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "ve-so-do",
        "name: ve-so-do\ntitle: Vẽ sơ đồ\ndescription: Dựng sơ đồ khối.\nkeywords: [sơ đồ]",
        "## Quy trình\n1. Chỉ dùng cú pháp mermaid.",
    );
    let (ctx, prompt) = cam_skills(&root).await;

    chay_pre_step(&ctx, vec![Message::user("vẽ sơ đồ cho tôi")]).await;
    assert!(prompt.assemble().contains("cú pháp mermaid"));

    // Step two: no new text message.
    chay_pre_step(&ctx, Vec::new()).await;
    assert!(
        prompt.assemble().contains("cú pháp mermaid"),
        "quy trình biến mất giữa lượt"
    );
}
