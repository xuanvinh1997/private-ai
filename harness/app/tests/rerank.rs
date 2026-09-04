//! Rerank defaults must agree across both languages: the Rust side so settings can show real numbers before
//! anything is changed, the Python side so a fresh install runs correctly without a `rerank` entry. Nothing in
//! either type system enforces that, so this test reads the Python file directly with regexes.

use std::path::PathBuf;

/// The file defining the service-side defaults.
fn config_py() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../services/rag/src/pai_rag_service/config.py")
}

/// The value of a module-level constant, e.g. `DEFAULT_RERANK_MODEL = "..."`.
fn hang_so(source: &str, name: &str) -> Option<String> {
    let line = source
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{name} = ")))?;
    let value = line.split_once(" = ")?.1.trim();
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// A field's default inside `class RerankConfig`; the class body is sliced out first, since `top_n` and
/// `candidates` also occur elsewhere and matching the wrong one makes the test green while the thing it guards is broken.
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

    // The four values the settings screen shows, taken from the Python constants so a change there turns this red.
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
    // The valid backends stay the two `set_rerank` clamps to; a third added in Python would be silently coerced to `onnx`.
    assert!(
        source.contains(r#"Literal["onnx", "http"]"#),
        "tập backend hợp lệ đã đổi — `set_rerank` chỉ nhận `onnx` và `http`"
    );
}

/// The reader must actually read: without this, a typo in `truong_rerank` would make the test above always green.
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
