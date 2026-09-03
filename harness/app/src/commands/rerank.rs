//! Bật/tắt và chỉnh bước xếp hạng lại.
//!
//! # Vì sao đây là một màn hình riêng chứ không nằm trong Nhà cung cấp
//!
//! Reranker mặc định không phải một endpoint — nó là một tệp `.onnx` chạy trong tiến
//! trình service. Nó không có địa chỉ máy chủ, không có khoá, và không health check được
//! bằng một request. Đặt nó cạnh danh sách provider sẽ cho người dùng một biểu mẫu mà
//! quá nửa số ô không có nghĩa.
//!
//! # Vì sao có một nút tắt
//!
//! Vì bước này **đắt**, và cái giá thì tuỳ máy. Đo trên CPU với `bge-reranker-v2-m3`:
//! khoảng 0,4 giây mỗi đoạn, nên 30 ứng viên là hơn mười giây cho mỗi câu hỏi. Trên GPU
//! cùng phép ấy nhỏ tới mức không cần nghĩ tới.
//!
//! Người dùng trên một máy không có GPU cần đường thoát, và đường thoát ấy phải **nói ra
//! nó đổi lấy gì**: tắt xếp hạng lại thì truy hồi vẫn chạy bằng RRF, chỉ là thứ tự kém đi
//! ở những câu hỏi cần hiểu nghĩa thay vì khớp từ.

use serde_json::json;
use tauri::State;

use crate::AppState;
use crate::protocol::RerankSetting;

/// Mặc định hiện lên khi người dùng chưa đặt gì.
///
/// **Phải khớp** với `RerankConfig` trong
/// `services/rag/src/pai_rag_service/config.py`. Không có gì trong hệ kiểu bắt chúng khớp
/// — nên có một bài kiểm chứng đọc thẳng tệp Python ấy và so từng giá trị. Xem
/// `app/tests/rerank.rs`.
const DEFAULT_BACKEND: &str = "onnx";
const DEFAULT_MODEL: &str = "viplao5/bge-reranker-v2-m3-onnx";
const DEFAULT_CANDIDATES: u32 = 30;
const DEFAULT_TOP_N: u32 = 8;
/// Tệp ONNX bên trong repo mặc định — repo chính chủ của BAAI đặt ở `onnx/model.onnx`,
/// bản export này đặt ở gốc.
const DEFAULT_ONNX_FILE: &str = "model.onnx";

#[tauri::command]
pub async fn rerank_setting(state: State<'_, AppState>) -> Result<RerankSetting, String> {
    let harness = state.harness().await?;
    let stored = harness.rag_config.rerank();

    let read_str = |key: &str, fallback: &str| -> String {
        stored
            .as_ref()
            .and_then(|found| found.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback)
            .to_string()
    };
    let read_u32 = |key: &str, fallback: u32| -> u32 {
        stored
            .as_ref()
            .and_then(|found| found.get(key))
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u32)
            .unwrap_or(fallback)
    };
    let enabled = stored
        .as_ref()
        .and_then(|found| found.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);

    let candidates = read_u32("candidates", DEFAULT_CANDIDATES);
    Ok(RerankSetting {
        enabled,
        backend: read_str("backend", DEFAULT_BACKEND),
        model: read_str("model", DEFAULT_MODEL),
        candidates,
        top_n: read_u32("top_n", DEFAULT_TOP_N),
        reason: reason_for(enabled, candidates),
    })
}

/// Câu nói ra cái giá đang trả, để công tắc không phải là một lựa chọn mù.
fn reason_for(enabled: bool, candidates: u32) -> Option<String> {
    if !enabled {
        return Some(
            "Đang tắt. Truy hồi vẫn chạy bằng cách hợp nhất từ khoá với vector, nhưng thứ \
             tự kém hơn ở những câu hỏi cần hiểu nghĩa thay vì khớp từ."
                .to_string(),
        );
    }
    // Con số này là ước lượng đo trên CPU, và nó cố ý nói "nếu chạy CPU" chứ không khẳng
    // định: ứng dụng không biết service đang chạy trên gì cho tới khi nó nạp model xong.
    // Log của service nói ra provider thật.
    let seconds = f64::from(candidates) * 0.4;
    Some(format!(
        "Chấm lại {candidates} đoạn cho mỗi câu hỏi. Nếu service chạy trên CPU thì mất \
         khoảng {seconds:.0} giây; trên GPU thì gần như tức thì. Hạ số ứng viên xuống là \
         cách nhanh nhất để bớt chờ."
    ))
}

#[tauri::command]
pub async fn set_rerank(
    enabled: bool,
    backend: String,
    model: String,
    candidates: u32,
    top_n: u32,
    state: State<'_, AppState>,
) -> Result<RerankSetting, String> {
    let harness = state.harness().await?;

    // Siết ở đây chứ không tin biểu mẫu: một `candidates` bằng 0 làm mọi lần tìm trả về
    // rỗng, và một `top_n` lớn hơn `candidates` là xin nhiều hơn số đã lấy về.
    let candidates = candidates.clamp(1, 200);
    let top_n = top_n.clamp(1, candidates);
    let backend = if backend == "http" { "http" } else { DEFAULT_BACKEND };
    let model = model.trim().to_string();
    let model = if model.is_empty() { DEFAULT_MODEL.to_string() } else { model };

    // `onnx_file` không có ô trên giao diện: nó là chi tiết bố cục của một repo
    // HuggingFace, và người dùng đổi model thì gần như luôn đổi sang một repo đặt tệp ở
    // gốc như bản mặc định. Ai cần khác thì sửa thẳng trong `rag-config.json`, và lần ghi
    // sau **giữ nguyên** giá trị ấy — đó là lý do nó được đọc lại chứ không viết cứng.
    let onnx_file = harness
        .rag_config
        .rerank()
        .and_then(|found| {
            found
                .get("onnx_file")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| DEFAULT_ONNX_FILE.to_string());

    harness
        .rag_config
        .write_rerank(json!({
            "enabled": enabled,
            "backend": backend,
            "model": model,
            "onnx_file": onnx_file,
            "candidates": candidates,
            "top_n": top_n,
        }))
        .map_err(|err| format!("không ghi được cấu hình xếp hạng lại: {err}"))?;

    // Không khởi động lại service: nó soi `mtime` của tệp cấu hình và tự nạp lại ở lời
    // gọi kế tiếp — xem `pai_rag_service.config.load`. Giết tiến trình con ở đây sẽ vứt
    // luôn phiên ONNX vừa nạp sẵn, tức là trả lại đúng cái cold boot vừa bỏ công tránh.
    Ok(RerankSetting {
        enabled,
        backend: backend.to_string(),
        model,
        candidates,
        top_n,
        reason: reason_for(enabled, candidates),
    })
}
