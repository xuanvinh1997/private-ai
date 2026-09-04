//! Speech recognition: choosing the model, and holding the microphone.
//!
//! Two features share one model here. Audio files in a document project go through the library's
//! ingest path; the microphone goes straight into the composer. Neither sends audio anywhere, and
//! the PCM never crosses this bridge -- the UI asks for dictation and receives text.

use std::path::PathBuf;

use pai_asr::{AsrConfig, DictationEvent};
use tauri::State;
use tauri::ipc::Channel;

use crate::AppState;
use crate::protocol::{AsrModelInfo, AsrSetting, DictationUpdate};

/// The settings as stored, without loading anything: opening the settings screen must not pay for
/// half a gigabyte of model. `probe_asr` is the button that does.
#[tauri::command]
pub async fn asr_setting(state: State<'_, AppState>) -> Result<AsrSetting, String> {
    let harness = state.harness().await?;
    let config = harness.rag_config.asr();
    Ok(view(&config, None, reason_for(&config, None)))
}

/// Choose the model, the language hint, and whether audio files enter the library at all.
#[tauri::command]
pub async fn set_asr(
    enabled: bool,
    model: String,
    language: String,
    state: State<'_, AppState>,
) -> Result<AsrSetting, String> {
    let harness = state.harness().await?;
    let config = AsrConfig {
        enabled,
        model: PathBuf::from(model.trim()),
        language: language.trim().to_string(),
    };
    harness
        .rag_config
        .write_asr(&config)
        .map_err(|err| format!("không ghi được cấu hình nhận dạng tiếng nói: {err}"))?;
    // The library also polls this file, but the composer's microphone does not; adopt it here so
    // dictation is right the moment the dialog closes.
    harness.asr.set_config(config.clone());
    Ok(view(&config, None, reason_for(&config, None)))
}

/// Load the chosen model and report what it turned out to be. Slow on purpose -- this is the only
/// answer to "will this file work" that is not a guess.
#[tauri::command]
pub async fn probe_asr(state: State<'_, AppState>) -> Result<AsrSetting, String> {
    let harness = state.harness().await?;
    let config = harness.rag_config.asr();
    match harness.asr.describe().await {
        Ok(info) => {
            let info = AsrModelInfo {
                arch: info.arch,
                variant: info.variant,
                backend: info.backend,
                streaming: info.streaming,
                languages: info.languages,
            };
            let reason = reason_for(&config, Some(&info));
            Ok(view(&config, Some(info), reason))
        }
        Err(error) => Ok(view(&config, None, Some(error.to_string()))),
    }
}

/// Start dictating into the composer. Every tick arrives on `on_update`; the command returns as
/// soon as the microphone is open, so the UI can enable its stop button.
#[tauri::command]
pub async fn start_dictation(
    on_update: Channel<DictationUpdate>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let harness = state.harness().await?;
    // One microphone, one dictation: a second start would open the device twice and interleave two
    // transcripts into the same box.
    if let Some(previous) = state.dictation.lock().take() {
        previous.cancel();
    }
    // Load before the device opens. `dictate` would otherwise do it inline -- on this runtime thread, for
    // as long as half a gigabyte takes to read -- and the microphone would come up seconds after the button
    // said it had. The UI holds the button disabled across this await.
    harness
        .asr
        .warm()
        .await
        .map_err(|error| error.to_string())?;
    let mut dictation = pai_asr::dictate(&harness.asr, pai_asr::Source::Microphone)
        .map_err(|error| error.to_string())?;
    *state.dictation.lock() = Some(dictation.control());

    tauri::async_runtime::spawn(async move {
        while let Some(event) = dictation.next().await {
            let update = translate(event);
            let last = matches!(update.kind.as_str(), "finished" | "failed");
            if let Err(error) = on_update.send(update) {
                // The window closed mid-sentence. Stop the microphone rather than record into a
                // channel nobody reads.
                tracing::debug!("không gửi được cập nhật đọc chính tả: {error}");
                dictation.cancel();
                break;
            }
            if last {
                break;
            }
        }
    });
    Ok(())
}

/// Stop and keep the text.
#[tauri::command]
pub fn stop_dictation(state: State<'_, AppState>) {
    if let Some(control) = state.dictation.lock().take() {
        control.stop();
    }
}

/// Stop and throw the text away.
#[tauri::command]
pub fn cancel_dictation(state: State<'_, AppState>) {
    if let Some(control) = state.dictation.lock().take() {
        control.cancel();
    }
}

fn translate(event: DictationEvent) -> DictationUpdate {
    let blank = DictationUpdate {
        kind: String::new(),
        committed: String::new(),
        tentative: String::new(),
        recorded_ms: 0,
        level: 0.0,
        device: None,
        streaming: false,
        text: None,
        error: None,
    };
    match event {
        DictationEvent::Started { device, streaming } => DictationUpdate {
            kind: "started".into(),
            device: Some(device),
            streaming,
            ..blank
        },
        DictationEvent::Text {
            committed,
            tentative,
            recorded_ms,
        } => DictationUpdate {
            kind: "text".into(),
            committed,
            tentative,
            recorded_ms,
            streaming: true,
            ..blank
        },
        DictationEvent::Recording { recorded_ms, level } => DictationUpdate {
            kind: "recording".into(),
            recorded_ms,
            level,
            ..blank
        },
        DictationEvent::Finished { text } => DictationUpdate {
            kind: "finished".into(),
            text: Some(text),
            ..blank
        },
        DictationEvent::Failed { message } => DictationUpdate {
            kind: "failed".into(),
            error: Some(message),
            ..blank
        },
    }
}

fn view(config: &AsrConfig, info: Option<AsrModelInfo>, reason: Option<String>) -> AsrSetting {
    AsrSetting {
        enabled: config.enabled,
        model: config.model.display().to_string(),
        language: config.language.clone(),
        info,
        reason,
    }
}

/// The sentence under the setting. It states what is currently true, including the two states that
/// look like breakage and are not: no model chosen, and a model that cannot dictate live.
fn reason_for(config: &AsrConfig, info: Option<&AsrModelInfo>) -> Option<String> {
    if config.model_path().is_none() {
        return Some(
            "Chưa chọn mô hình. Chọn một tệp .gguf để đọc tệp âm thanh trong dự án tài liệu \
             và để đọc chính tả bằng micro."
                .into(),
        );
    }
    if !config.enabled {
        return Some(
            "Đang tắt cho thư viện: tệp âm thanh trong dự án sẽ bị bỏ qua thay vì đọc thành \
             văn bản. Đọc chính tả bằng micro vẫn chạy."
                .into(),
        );
    }
    match info {
        Some(info) if !info.streaming => Some(format!(
            "`{}` không đọc theo dòng: chữ hiện ra khi bạn dừng ghi, không phải khi đang nói.",
            info.variant
        )),
        Some(info) => Some(format!(
            "Chạy `{}` trên {}. Đọc được {} ngôn ngữ.",
            info.variant,
            info.backend,
            info.languages.len()
        )),
        None => None,
    }
}
