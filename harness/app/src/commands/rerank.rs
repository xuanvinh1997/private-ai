//! Toggling and tuning the optional HTTP reranking step.

use serde_json::json;
use tauri::State;

use crate::AppState;
use crate::protocol::RerankSetting;

/// Rust ships no local reranker model, so a fresh install keeps the optional HTTP step off.
const DEFAULT_BACKEND: &str = "http";
const DEFAULT_MODEL: &str = "";
const DEFAULT_CANDIDATES: u32 = 30;
const DEFAULT_TOP_N: u32 = 8;

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
    let backend = read_str("backend", DEFAULT_BACKEND);
    let supported = backend == DEFAULT_BACKEND;
    let enabled = stored
        .as_ref()
        .and_then(|found| found.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        && supported;

    let candidates = read_u32("candidates", DEFAULT_CANDIDATES);
    Ok(RerankSetting {
        enabled,
        backend: DEFAULT_BACKEND.to_string(),
        url: if supported {
            read_str("url", "")
        } else {
            String::new()
        },
        model: if supported {
            read_str("model", DEFAULT_MODEL)
        } else {
            DEFAULT_MODEL.to_string()
        },
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
        "Gửi {candidates} đoạn tới máy chủ rerank HTTP cho mỗi câu hỏi. Hạ số ứng viên \
         xuống là cách nhanh nhất để giảm độ trễ."
    ))
}

#[tauri::command]
pub async fn set_rerank(
    enabled: bool,
    backend: String,
    url: String,
    model: String,
    candidates: u32,
    top_n: u32,
    state: State<'_, AppState>,
) -> Result<RerankSetting, String> {
    let harness = state.harness().await?;

    // Clamp here rather than trusting the form: `candidates` of 0 empties every search, and `top_n` above it asks for more than was fetched.
    let candidates = candidates.clamp(1, 200);
    let top_n = top_n.clamp(1, candidates);
    let _ = backend;
    let backend = DEFAULT_BACKEND;
    let url = url.trim().trim_end_matches('/').to_string();
    if enabled && url.is_empty() {
        return Err("hãy nhập URL máy chủ rerank trước khi bật".into());
    }
    let model = model.trim().to_string();
    let model = if model.is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        model
    };

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
            "url": url,
            "api_key": preserved("api_key"),
            "candidates": candidates,
            "top_n": top_n,
        }))
        .map_err(|err| format!("không ghi được cấu hình xếp hạng lại: {err}"))?;

    // Native RAG watches config `mtime`; the next search observes this write.
    Ok(RerankSetting {
        enabled,
        backend: backend.to_string(),
        url,
        model,
        candidates,
        top_n,
        reason: reason_for(enabled, candidates),
    })
}
