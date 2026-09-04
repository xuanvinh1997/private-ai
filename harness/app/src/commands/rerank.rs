//! Toggling and tuning the bundled local ONNX reranking step.

use serde_json::json;
use tauri::State;

use crate::AppState;
use crate::protocol::RerankSetting;

const DEFAULT_BACKEND: &str = "onnx";
const DEFAULT_MODEL: &str = "BAAI/bge-reranker-v2-m3";
const DEFAULT_CANDIDATES: u32 = 30;
const DEFAULT_TOP_N: u32 = 8;

#[tauri::command]
pub async fn rerank_setting(state: State<'_, AppState>) -> Result<RerankSetting, String> {
    let harness = state.harness().await?;
    let stored = harness.rag_config.rerank();

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
        .unwrap_or(false);

    let candidates = read_u32("candidates", DEFAULT_CANDIDATES);
    Ok(RerankSetting {
        enabled,
        backend: DEFAULT_BACKEND.to_string(),
        model: DEFAULT_MODEL.to_string(),
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
    Some(format!(
        "Chấm {candidates} đoạn bằng BGE reranker ONNX ngay trên máy cho mỗi câu hỏi. \
         Hạ số ứng viên xuống là cách nhanh nhất để giảm độ trễ CPU."
    ))
}

#[tauri::command]
pub async fn set_rerank(
    enabled: bool,
    candidates: u32,
    top_n: u32,
    state: State<'_, AppState>,
) -> Result<RerankSetting, String> {
    let harness = state.harness().await?;

    // Clamp here rather than trusting the form: `candidates` of 0 empties every search, and `top_n` above it asks for more than was fetched.
    let candidates = candidates.clamp(1, 200);
    let top_n = top_n.clamp(1, candidates);
    let backend = DEFAULT_BACKEND;
    let model = DEFAULT_MODEL;

    let previous = harness.rag_config.rerank();
    let preserved = |key: &str| {
        previous
            .as_ref()
            .and_then(|found| found.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    harness
        .rag_config
        .write_rerank(json!({
            "enabled": enabled,
            "backend": backend,
            "model": model,
            "path": std::env::var("PAI_RERANK_MODEL_DIR")
                .unwrap_or_else(|_| preserved("path")),
            "candidates": candidates,
            "top_n": top_n,
        }))
        .map_err(|err| format!("không ghi được cấu hình xếp hạng lại: {err}"))?;

    // Native RAG watches config `mtime`; the next search observes this write.
    Ok(RerankSetting {
        enabled,
        backend: backend.to_string(),
        model: model.to_string(),
        candidates,
        top_n,
        reason: reason_for(enabled, candidates),
    })
}
