//! Tiết lộ dần, và việc chọn skill.
//!
//! Bài quan trọng nhất ở đây là bài về ba tầng. Nếu toàn văn hướng dẫn lọt vào prompt khi
//! chưa được chọn thì cơ chế mất hết ý nghĩa: một trăm skill sẽ tốn một trăm tài liệu chứ
//! không phải một trăm dòng, và đó đúng là thứ nó sinh ra để tránh.

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

    // Tầng một: luôn có tên và mô tả, không có thân.
    let catalog = registry.catalog().expect("có danh mục");
    assert!(catalog.contains("tra-cuu-hop-dong"));
    assert!(catalog.contains("Tìm điều khoản"));
    // Kiểm bằng một câu chỉ có trong thân: tiêu đề của chính danh mục cũng chứa chữ
    // "Quy trình", nên so khớp cụm đó là kiểm nhầm cái tiêu đề.
    assert!(
        !catalog.contains("đúng cụm từ người dùng nêu"),
        "thân hướng dẫn lọt vào tầng một:\n{catalog}"
    );
    assert!(
        !catalog.contains("mau.md"),
        "tên tệp phụ chỉ xuất hiện ở tầng hai"
    );

    // Chưa chọn thì tầng hai trống.
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

    // Gõ không dấu vẫn phải trúng — nếu không thì cơ chế chọn gần như không bao giờ chạy.
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

/// Trùng **một** từ thông dụng không kéo được cả sổ skill vào lượt.
///
/// Không có sàn tương đối thì một câu hỏi dài kéo theo mọi skill có chung một từ với nó,
/// và tiết lộ dần mất sạch ý nghĩa đúng vào lúc nó cần nhất — lúc người dùng gõ nhiều.
#[test]
fn san_tuong_doi_gat_bo_cai_chi_trung_mot_tu() {
    let root = TempDir::new().expect("thư mục tạm");
    write_skill(
        &root,
        "chinh",
        "name: doi-chieu-hop-dong\ntitle: Đối chiếu hợp đồng\ndescription: So hai bản hợp đồng.\nkeywords: [hợp đồng, đối chiếu]",
        "## Quy trình\n1. So từng điều.",
    );
    // Chỉ dính đúng một từ khoá — đủ qua ngưỡng tuyệt đối, nhưng quá xa cái đứng đầu.
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

/// Một từ trong phần mô tả không phải một lời gọi.
///
/// Từ trong mô tả đáng nửa điểm chứ không phải hai điểm, vì mô tả là một câu văn xuôi:
/// cho nó cùng trọng số với từ khoá thì mọi skill có chữ "tài liệu" trong mô tả đều được
/// chọn mỗi khi người dùng nhắc tới tài liệu.
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

    // "thanh" là một từ dài trong mô tả, và chỉ thế thôi.
    assert!(
        registry.select("giúp tôi làm thành phẩm").is_empty(),
        "một từ trong mô tả mà đủ điểm thì mọi câu hỏi dài đều kéo theo skill này"
    );
    // Còn một từ khoá thì đủ.
    assert_eq!(registry.select("vẽ biểu đồ").len(), 1);
}

/// Gói quét sau **thay thế** gói trùng tên, không đứng cạnh nó.
///
/// Đây là cách một gói của người dùng đè lên gói dựng sẵn. Hai gói cùng tên cùng tồn tại
/// thì `select` trả về một cái tên trỏ tới hai thân khác nhau, và cái nào vào prompt là
/// chuyện của thứ tự trong một `Vec`.
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

/// Tên đi vào prompt và vào tên thư mục, nên nó hẹp — và gói sai tên bị bỏ, không được sửa hộ.
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

/// Cắm plugin lên một `Context` sống lâu hơn lời gọi này.
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

/// Toàn văn hướng dẫn chỉ vào prompt **sau khi** được chọn, và tệp đi kèm chỉ vào bằng tên.
///
/// Bài `ba_tang_tiet_lo_dan` khoá chiều "chưa chọn thì chưa có"; bài này khoá chiều còn
/// lại. Thiếu nó thì một lỗi làm tầng hai không bao giờ vào prompt sẽ đi qua cả bộ kiểm
/// mà không ai thấy — mọi bài còn lại đều xanh khi khối ấy trống rỗng.
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

/// Bước sau trong cùng một lượt không mang message mới, và quy trình phải ở nguyên đó.
///
/// Xoá lựa chọn khi không có văn bản mới nghĩa là hướng dẫn biến mất giữa chừng, đúng lúc
/// mô hình đang đi theo nó — nó gọi tool ở bước một rồi mất luật ở bước hai.
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

    // Bước hai: không có message text nào mới.
    chay_pre_step(&ctx, Vec::new()).await;
    assert!(
        prompt.assemble().contains("cú pháp mermaid"),
        "quy trình biến mất giữa lượt"
    );
}
