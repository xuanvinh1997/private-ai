//! How documents are cut before embedding. Two numbers, kept on their own screen section because they are
//! the only settings here that invalidate work already done: changing either re-cuts and re-embeds the
//! whole library on the next sync.

use serde_json::json;
use tauri::State;

use crate::AppState;
use crate::protocol::ChunkSetting;

/// Matches `ChunkConfig::default` in `pai-rag`; duplicated rather than shared because the service treats an
/// absent key as "use my default", and the UI must show the same number the service would then use.
const DEFAULT_SIZE: u32 = 1_400;
const DEFAULT_OVERLAP: u32 = 180;

/// Room to work in either direction: a paragraph at the bottom, a whole section at the top. Past this the
/// numbers stop meaning what the screen says they mean -- a chunk larger than an embedding model's window is
/// silently truncated, and one below a sentence retrieves fragments nobody can read.
const MIN_SIZE: u32 = 200;
const MAX_SIZE: u32 = 8_000;

#[tauri::command]
pub async fn chunk_setting(state: State<'_, AppState>) -> Result<ChunkSetting, String> {
    let harness = state.harness().await?;
    let stored = harness.rag_config.chunk();
    let read = |key: &str, fallback: u32| -> u32 {
        stored
            .as_ref()
            .and_then(|found| found.get(key))
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u32)
            .unwrap_or(fallback)
    };
    let size = read("size", DEFAULT_SIZE);
    let overlap = read("overlap", DEFAULT_OVERLAP);
    Ok(ChunkSetting {
        size,
        overlap,
        reason: reason_for(size, overlap),
    })
}

/// What these two numbers buy and cost, in the terms the user is choosing between: how much context arrives
/// with a hit, against how precisely a hit points at the answer.
fn reason_for(size: u32, overlap: u32) -> Option<String> {
    Some(format!(
        "Mỗi đoạn khoảng {size} ký tự, lặp {overlap} ký tự với đoạn trước. Đoạn dài thì câu \
         trả lời có nhiều ngữ cảnh hơn nhưng trích dẫn kém sát; đoạn ngắn thì trích dẫn sát \
         hơn nhưng dễ mất mạch. Phần lặp giữ cho một câu bị cắt ngang vẫn tìm ra được."
    ))
}

/// Persist both numbers, clamped. `overlap` is capped at half of `size`: at more than that consecutive
/// chunks are mostly the same text, which doubles the vectors stored and retrieves the same passage twice.
#[tauri::command]
pub async fn set_chunk(
    size: u32,
    overlap: u32,
    state: State<'_, AppState>,
) -> Result<ChunkSetting, String> {
    let harness = state.harness().await?;
    let size = size.clamp(MIN_SIZE, MAX_SIZE);
    let overlap = overlap.min(size / 2);

    harness
        .rag_config
        .write_chunk(json!({ "size": size, "overlap": overlap }))
        .map_err(|err| format!("không ghi được cấu hình cắt đoạn: {err}"))?;

    // Native RAG watches config `mtime`: it drops every fingerprint on the next sync and re-cuts the library.
    Ok(ChunkSetting {
        size,
        overlap,
        reason: reason_for(size, overlap),
    })
}
