//! Toggling and tuning the reranking step. It gets its own screen because the default reranker is an `.onnx`
//! file inside the service, with no host, key or health check -- half a provider form would be meaningless.
//! It can be switched off because it is expensive on CPU, and the switch must say what that costs.

use serde_json::json;
use tauri::State;

use crate::AppState;
use crate::protocol::RerankSetting;

/// Defaults shown before the user sets anything; they must match `RerankConfig` in
/// `services/rag/src/pai_rag_service/config.py`, which a test enforces by reading that file.
const DEFAULT_BACKEND: &str = "onnx";
const DEFAULT_MODEL: &str = "viplao5/bge-reranker-v2-m3-onnx";
const DEFAULT_CANDIDATES: u32 = 30;
const DEFAULT_TOP_N: u32 = 8;
/// The ONNX file inside the default repo; BAAI's own repo puts it under `onnx/`, this export at the root.
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

/// The sentence stating the cost, so the toggle is not a blind choice.
fn reason_for(enabled: bool, candidates: u32) -> Option<String> {
    if !enabled {
        return Some(
            "Đang tắt. Truy hồi vẫn chạy bằng cách hợp nhất từ khoá với vector, nhưng thứ \
             tự kém hơn ở những câu hỏi cần hiểu nghĩa thay vì khớp từ."
                .to_string(),
        );
    }
    // A CPU-measured estimate, hedged on purpose: the app cannot know what the service runs on until it loads the model.
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

    // Clamp here rather than trusting the form: `candidates` of 0 empties every search, and `top_n` above it asks for more than was fetched.
    let candidates = candidates.clamp(1, 200);
    let top_n = top_n.clamp(1, candidates);
    let backend = if backend == "http" { "http" } else { DEFAULT_BACKEND };
    let model = model.trim().to_string();
    let model = if model.is_empty() { DEFAULT_MODEL.to_string() } else { model };

    // `onnx_file` has no form field: it is a HuggingFace repo layout detail. Anyone who needs a different one
    // edits `rag-config.json`, and it is read back rather than hard-coded so later writes preserve it.
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

    // Do not restart the service: it watches the config `mtime` and reloads itself, and killing it would
    // discard the warm ONNX session -- exactly the cold start we were avoiding.
    Ok(RerankSetting {
        enabled,
        backend: backend.to_string(),
        model,
        candidates,
        top_n,
        reason: reason_for(enabled, candidates),
    })
}
