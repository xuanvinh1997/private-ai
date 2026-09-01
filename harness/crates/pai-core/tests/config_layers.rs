//! Áp lớp cấu hình.
//!
//! Cây plugin là thứ quyết định ứng dụng gồm những gì, nên một lỗi im lặng ở đây hiện ra
//! dưới dạng "tính năng kia biến mất" chứ không dưới dạng một thông báo lỗi. Vì vậy mọi
//! thao tác không khớp đều là lỗi có tên, không phải một lần bỏ qua.

use pai_core::{ConfigError, Layer, Patch, Row, compose};
use serde_json::json;

fn row(id: &str, plugin: &str) -> Row {
    Row {
        id: id.into(),
        plugin: plugin.into(),
        config: json!({}),
        disabled: false,
    }
}

#[test]
fn lop_tren_thay_ca_khoi_cau_hinh_chu_khong_tron_vao() {
    let base = Layer::base(
        "nen",
        vec![Row {
            config: json!({ "a": 1, "b": 2 }),
            ..row("fs", "fs")
        }],
    );
    let user = Layer::new(
        "nguoi-dung",
        vec![Patch::Replace {
            id: "fs".into(),
            config: json!({ "a": 9 }),
        }],
    );

    let composed = compose(&[base, user]).expect("áp được");
    // Trộn thì `b` còn sót lại, và người viết bản vá không có cách nào xoá nó.
    assert_eq!(composed.rows[0].config, json!({ "a": 9 }));
}

#[test]
fn tat_chu_khong_xoa_va_van_nhin_thay_trong_dump() {
    let base = Layer::base("nen", vec![row("shell", "shell"), row("fs", "fs")]);
    let user = Layer::new("nguoi-dung", vec![Patch::Disable { id: "shell".into() }]);

    let composed = compose(&[base, user]).expect("áp được");
    assert_eq!(composed.active().count(), 1);
    // Một hàng vắng mặt sẽ lặng lẽ sống lại vào ngày ai đó đổi thứ tự lớp; một hàng tắt
    // thì luôn nhìn thấy được.
    assert!(composed.dump().contains("shell: shell [tắt]"));
}

#[test]
fn dump_noi_ro_ai_da_dung_vao_hang_nao() {
    let base = Layer::base("nen.yaml", vec![row("fs", "fs")]);
    let mid = Layer::new(
        "ho-so.yaml",
        vec![Patch::Replace {
            id: "fs".into(),
            config: json!({ "roots": [] }),
        }],
    );
    let user = Layer::new("nha.yaml", vec![Patch::Disable { id: "fs".into() }]);

    let composed = compose(&[base, mid, user]).expect("áp được");
    let dump = composed.dump();
    assert!(
        dump.contains("nen.yaml → ho-so.yaml → nha.yaml"),
        "thiếu dấu vết:\n{dump}"
    );
}

#[test]
fn chen_trung_id_la_loi_chu_khong_phai_ghi_de_im_lang() {
    let base = Layer::base("nen", vec![row("fs", "fs")]);
    let other = Layer::base("khac", vec![row("fs", "fs-khac")]);

    // Người viết lớp gần như chắc chắn đang định `replace`; nuốt cái này là để họ tìm cả
    // buổi xem vì sao cấu hình của mình không có tác dụng.
    let err = compose(&[base, other]).expect_err("phải là lỗi");
    assert!(matches!(err, ConfigError::Duplicate { .. }), "{err}");
}

#[test]
fn nham_vao_hang_khong_ton_tai_la_loi_co_ten() {
    let base = Layer::base("nen", vec![row("fs", "fs")]);
    let user = Layer::new("nguoi-dung", vec![Patch::Disable { id: "shel".into() }]);

    let err = compose(&[base, user]).expect_err("phải là lỗi");
    // Lỗi phải nêu cả nơi viết lẫn tên đã gõ — gõ nhầm một chữ là chuyện thường, và
    // "không có gì xảy ra" là câu trả lời tệ nhất cho nó.
    let text = err.to_string();
    assert!(
        text.contains("nguoi-dung") && text.contains("shel"),
        "{text}"
    );
}

#[test]
fn thu_tu_hang_giu_theo_thu_tu_chen() {
    let base = Layer::base("nen", vec![row("a", "a"), row("b", "b")]);
    let more = Layer::base("them", vec![row("c", "c")]);

    let composed = compose(&[base, more]).expect("áp được");
    let ids: Vec<&str> = composed.rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}
