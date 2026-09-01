//! Tiết lộ dần, và việc chọn skill.
//!
//! Bài quan trọng nhất ở đây là bài về ba tầng. Nếu toàn văn hướng dẫn lọt vào prompt khi
//! chưa được chọn thì cơ chế mất hết ý nghĩa: một trăm skill sẽ tốn một trăm tài liệu chứ
//! không phải một trăm dòng, và đó đúng là thứ nó sinh ra để tránh.

use std::sync::Arc;

use pai_agent::{Prompt, SkillRegistry, SkillsPlugin, SystemPrompt};
use pai_core::{Context, Plugin};
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
