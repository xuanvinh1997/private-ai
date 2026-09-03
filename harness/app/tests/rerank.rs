//! Mặc định của xếp hạng lại phải giống nhau ở hai ngôn ngữ.
//!
//! Giá trị mặc định nằm ở hai chỗ, và cả hai đều cần thiết:
//!
//! - `app/src/commands/rerank.rs` — để màn hình Cài đặt hiện được **số thật** trước khi
//!   người dùng đổi gì. Hiện một ô trống hay chữ "mặc định" thì họ không biết mình sắp
//!   đổi từ đâu sang đâu.
//! - `services/rag/src/pai_rag_service/config.py` — để service chạy đúng khi tệp cấu hình
//!   chưa có mục `rerank`, tức là ở mọi lần cài mới.
//!
//! Không có gì trong hệ kiểu bắt hai chỗ ấy khớp. Nếu chúng lệch, triệu chứng là màn hình
//! Cài đặt nói một đằng còn service làm một nẻo — không lỗi, không cảnh báo, chỉ là hai
//! con số khác nhau mà không ai đối chiếu. Bài này là thứ đối chiếu chúng.
//!
//! Đọc thẳng tệp Python bằng regex chứ không chạy Python: bộ test của Rust không nên đòi
//! một môi trường ảo dựng xong mới chạy được, và thứ cần lấy ở đây là bốn hằng số nằm
//! ngay trên bề mặt tệp.

use std::path::PathBuf;

/// Tệp định nghĩa mặc định phía service.
fn config_py() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../services/rag/src/pai_rag_service/config.py")
}

/// Giá trị của một hằng số cấp module, ví dụ `DEFAULT_RERANK_MODEL = "..."`.
fn hang_so(source: &str, name: &str) -> Option<String> {
    let line = source
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name} = ")))?;
    let value = line.split_once(" = ")?.1.trim();
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// Giá trị mặc định của một trường trong `class RerankConfig`.
///
/// Cắt lấy đúng thân lớp trước khi tìm: `top_n` và `candidates` là những cái tên có thể
/// xuất hiện ở lớp khác, và lấy nhầm một giá trị trông giống là kiểu hỏng tệ nhất mà một
/// bài kiểm chứng có thể mắc — nó xanh trong khi thứ nó canh đã sai.
fn truong_rerank(source: &str, name: &str) -> Option<String> {
    let start = source.find("class RerankConfig(BaseModel):")?;
    let rest = &source[start..];
    let end = rest[1..].find("\nclass ").map(|at| at + 1).unwrap_or(rest.len());
    let body = &rest[..end];

    let line = body
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name}:")))?;
    let value = line.split_once('=')?.1.trim();
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_string())
}

#[test]
fn mac_dinh_rerank_khop_giua_rust_va_python() {
    let path = config_py();
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "không đọc được `{}`: {err}. Bài này cần cây mã nguồn đầy đủ — nếu service đã \
             chuyển chỗ thì sửa `config_py()`.",
            path.display()
        )
    });

    // Bốn giá trị mà màn hình Cài đặt hiện ra. Lấy từ chính hằng số Python để một lần đổi
    // bên đó làm bài này đỏ, thay vì trôi đi im lặng.
    assert_eq!(
        hang_so(&source, "DEFAULT_RERANK_MODEL").as_deref(),
        Some("viplao5/bge-reranker-v2-m3-onnx"),
        "model mặc định đã đổi bên Python — cập nhật `DEFAULT_MODEL` trong commands/rerank.rs"
    );
    assert_eq!(
        hang_so(&source, "DEFAULT_RERANK_ONNX_FILE").as_deref(),
        Some("model.onnx"),
        "tệp ONNX mặc định đã đổi — cập nhật `DEFAULT_ONNX_FILE` trong commands/rerank.rs"
    );
    assert_eq!(
        truong_rerank(&source, "candidates").as_deref(),
        Some("30"),
        "số ứng viên mặc định đã đổi — cập nhật `DEFAULT_CANDIDATES` trong commands/rerank.rs"
    );
    assert_eq!(
        truong_rerank(&source, "top_n").as_deref(),
        Some("8"),
        "top_n mặc định đã đổi — cập nhật `DEFAULT_TOP_N` trong commands/rerank.rs"
    );
    assert_eq!(
        truong_rerank(&source, "backend").as_deref(),
        Some("onnx"),
        "backend mặc định đã đổi — cập nhật `DEFAULT_BACKEND` trong commands/rerank.rs"
    );
    // Và tập nhánh hợp lệ vẫn là hai cái mà `set_rerank` biết siết về. Thêm một nhánh
    // thứ ba bên Python mà quên bên này thì giao diện lặng lẽ nắn nó về `onnx`.
    assert!(
        source.contains(r#"Literal["onnx", "http"]"#),
        "tập backend hợp lệ đã đổi — `set_rerank` chỉ nhận `onnx` và `http`"
    );
}

/// Phép đọc phải thật sự **đọc**, không phải luôn trả về `None` rồi so `None` với `None`.
///
/// Không có bài này thì một lỗi chính tả trong `truong_rerank` biến bài trên thành một
/// bài luôn xanh.
#[test]
fn phep_doc_khong_im_lang_tra_ve_rong() {
    let source = std::fs::read_to_string(config_py()).expect("đọc config.py");
    assert!(hang_so(&source, "DEFAULT_RERANK_MODEL").is_some());
    assert!(truong_rerank(&source, "candidates").is_some());
    assert!(
        hang_so(&source, "KHONG_CO_HANG_SO_NAY").is_none(),
        "một cái tên không tồn tại phải trả về None"
    );
    assert!(
        truong_rerank(&source, "khong_co_truong_nay").is_none(),
        "một trường không tồn tại phải trả về None"
    );
}
