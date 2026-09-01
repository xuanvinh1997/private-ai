//! Tiền tố, và những cái tên không được phép tồn tại.
//!
//! Đọc như đọc luật: mỗi bài khoá đúng một câu, và câu đó nằm ở dòng doc của bài.

use pai_mcp::{ServerConfig, is_external, namespace, qualify, remote_of};
use pai_tools::ToolName;

/// Tiền tố đặt vào ở dạng `ext.<server>.`, và cắt ra trả lại đúng cái tên server đã công bố.
#[test]
fn tien_to_dat_va_cat_dung_chieu() {
    let name = qualify("github", "search_issues");
    assert_eq!(name.as_str(), "ext.github.search_issues");
    assert_eq!(remote_of("github", &name), Some("search_issues"));
    assert_eq!(namespace("github"), "ext.github");
}

/// Tên từ xa có dấu chấm đi qua nguyên vẹn: phép cắt không đụng tới phần đuôi.
#[test]
fn ten_tu_xa_co_dau_cham_khong_bi_dong_vao() {
    let name = qualify("srv", "rag.vector.search");
    assert_eq!(name.as_str(), "ext.srv.rag.vector.search");
    assert_eq!(remote_of("srv", &name), Some("rag.vector.search"));
}

/// Cắt đúng **một** lần và đúng ở đầu — một server bên thứ ba tự đặt tên tool là
/// `ext.other.thing` không mượn được danh tính của server `other`.
#[test]
fn cat_tien_to_chi_cat_mot_lan() {
    let name = qualify("srv", "ext.other.thing");
    assert_eq!(name.as_str(), "ext.srv.ext.other.thing");
    assert_eq!(remote_of("srv", &name), Some("ext.other.thing"));
    // Và nó không phải là tool của `other`.
    assert_eq!(remote_of("other", &name), None);
    assert_ne!(name, qualify("other", "thing"));
}

/// Hỏi sai server thì không trả về gì — không có đường nào để một cái tên trôi sang server khác.
#[test]
fn khong_cat_duoc_tien_to_cua_server_khac() {
    let name = qualify("alpha", "ping");
    assert_eq!(remote_of("beta", &name), None);
    // Tiền tố khớp một phần cũng không tính: `al` không phải `alpha`.
    assert_eq!(remote_of("al", &name), None);
}

/// Tool nội bộ không bao giờ bị coi là tool ngoài, và ngược lại.
#[test]
fn phan_biet_duoc_tool_trong_voi_tool_ngoai() {
    assert!(is_external(&qualify("srv", "read")));
    assert!(!is_external(&ToolName::new("read")));
    // `extra` bắt đầu bằng `ext` nhưng không phải `ext.` — chỗ này là nơi một phép so
    // sánh cẩu thả sẽ nhận nhầm.
    assert!(!is_external(&ToolName::new("extra.thing")));
}

/// Tên server đi vào danh tính tool, nên nó bị kiểm như một danh tính chứ không như một nhãn.
#[test]
fn ten_server_bi_kiem() {
    assert!(ServerConfig::stdio("github", "npx").validate().is_ok());
    assert!(ServerConfig::stdio("my_server-2", "npx").validate().is_ok());

    // Rỗng: `ext..search` không định danh server nào cả.
    assert!(ServerConfig::stdio("", "npx").validate().is_err());
    // Dấu chấm: `a.b` + tool `c` và `a` + tool `b.c` ra cùng một tên đầy đủ.
    assert!(ServerConfig::stdio("a.b", "npx").validate().is_err());
    // `__`: phá mất tính khả nghịch của phép chiếu tên sang dạng wire.
    assert!(ServerConfig::stdio("a__b", "npx").validate().is_err());
    // Khoảng trắng, dấu nháy, gạch chéo — tất cả đều là ký tự của một cái tên không kiểm được.
    assert!(ServerConfig::stdio("a b", "npx").validate().is_err());
    assert!(ServerConfig::stdio("a/b", "npx").validate().is_err());
}

/// Một cái tên đã qua kiểm cấu hình thì cũng qua được kiểm của sổ đăng ký.
///
/// Hai tầng này ở hai crate khác nhau và không gọi nhau; bài này khoá chúng lại với nhau.
#[test]
fn ten_qua_kiem_cau_hinh_thi_qua_duoc_so_dang_ky() {
    for server in ["github", "my_server-2", "a1"] {
        assert!(ServerConfig::stdio(server, "npx").validate().is_ok());
        assert!(qualify(server, "search").round_trips());
    }
}

/// Transport cũng bị kiểm: một lệnh rỗng hay một url không phải http là cấu hình sai, không
/// phải một lần nối hỏng ở đâu đó trong nền.
#[test]
fn transport_bi_kiem() {
    assert!(ServerConfig::stdio("srv", "   ").validate().is_err());
    assert!(ServerConfig::http("srv", "ftp://x").validate().is_err());
    assert!(ServerConfig::http("srv", "ws://x").validate().is_err());
    assert!(
        ServerConfig::http("srv", "https://example.com/mcp")
            .validate()
            .is_ok()
    );
}

/// Cấu hình đọc được từ JSON, và mặc định là thứ người dùng không phải gõ.
#[test]
fn cau_hinh_doc_duoc_tu_json() {
    let stdio: ServerConfig = serde_json::from_str(
        r#"{"name":"github","transport":"stdio","command":"npx","args":["-y","x"]}"#,
    )
    .expect("đọc được cấu hình stdio");
    assert!(stdio.enabled);
    assert_eq!(stdio.connect_timeout().as_secs(), 20);
    assert!(stdio.validate().is_ok());

    let http: ServerConfig = serde_json::from_str(
        r#"{"name":"remote","transport":"http","url":"https://example.com/mcp",
            "headers":{"authorization":"Bearer x"},"max_retries":1}"#,
    )
    .expect("đọc được cấu hình http");
    assert_eq!(http.max_retries, 1);
    assert!(http.validate().is_ok());
}
