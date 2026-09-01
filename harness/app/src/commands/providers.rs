//! Nhà cung cấp mô hình: liệt kê, sửa, thử, và đổi cái đang dùng.

use pai_llm::{ProviderConfig, ProviderKind};
use pai_providers::{ProviderInput, StoredProvider};
use tauri::State;

use crate::AppState;
use crate::protocol::{
    ModelChoice, ProviderInputWire, ProviderPreset, ProviderProbe, ProviderView,
};

/// Chuỗi trên dây thành loại provider.
///
/// Từ chối thay vì lùi về một mặc định: một `kind` gõ sai mà lặng lẽ thành `openai` sẽ gửi
/// request sai giao thức tới một máy chủ Ollama, và thông báo lỗi khi đó nói về JSON chứ
/// không nói về cấu hình.
fn kind(raw: &str) -> Result<ProviderKind, String> {
    ProviderKind::parse(raw).ok_or_else(|| format!("loại nhà cung cấp không hợp lệ: `{raw}`"))
}

fn view(stored: &StoredProvider) -> ProviderView {
    ProviderView {
        id: stored.id().to_string(),
        name: stored.config.name.clone(),
        kind: stored.config.kind.as_str().to_string(),
        base_url: stored.config.base_url.clone(),
        has_key: stored.has_key(),
        enabled: stored.config.enabled,
        on_device: stored.config.on_device(),
        active: stored.active,
        model: stored.model.clone(),
    }
}

fn input(wire: ProviderInputWire) -> Result<ProviderInput, String> {
    let mut built = ProviderInput::create(wire.name, kind(&wire.kind)?, wire.base_url);
    built.id = wire.id;
    built.enabled = wire.enabled;
    built.model = wire.model;
    built.api_key = wire.api_key;
    Ok(built)
}

#[tauri::command]
pub async fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderView>, String> {
    let harness = state.harness().await?;
    Ok(harness
        .providers
        .list()
        .map_err(|err| err.to_string())?
        .iter()
        .map(view)
        .collect())
}

#[tauri::command]
pub async fn provider_presets(state: State<'_, AppState>) -> Result<Vec<ProviderPreset>, String> {
    // Không cần harness, nhưng vẫn nhận `state` để mọi lệnh trong nhóm có cùng hình dạng —
    // và để danh mục không trở thành thứ duy nhất trả lời được khi lõi chưa dựng nổi.
    let _ = state;
    Ok(pai_providers::PRESETS
        .iter()
        .map(|preset| ProviderPreset {
            id: preset.id.to_string(),
            name: preset.name.to_string(),
            kind: preset.kind.as_str().to_string(),
            base_url: preset.base_url.to_string(),
            needs_key: preset.needs_key,
            on_device: preset.on_device,
            default_model: preset.default_model.map(str::to_string),
            homepage: preset.homepage.to_string(),
            hint: preset.hint.to_string(),
        })
        .collect())
}

/// Lưu một provider, rồi **áp lại cái đang hoạt động**.
///
/// Áp lại kể cả khi hàng vừa lưu không phải hàng đang hoạt động: sửa URL của chính
/// provider đang chạy mà không áp lại thì mọi request tiếp theo vẫn bay tới máy chủ cũ, và
/// không có gì trên màn hình nói rằng nó đang làm vậy.
#[tauri::command]
pub async fn save_provider(
    input: ProviderInputWire,
    state: State<'_, AppState>,
) -> Result<ProviderView, String> {
    let harness = state.harness().await?;
    let saved = harness
        .providers
        .save(self::input(input)?)
        .await
        .map_err(|err| err.to_string())?;
    // Áp lại có thể hỏng — chưa nối được máy chủ mới chẳng hạn — nhưng hàng **đã lưu**
    // rồi, và báo lỗi ở đây sẽ khiến giao diện tưởng việc lưu thất bại rồi hiện lại giá
    // trị cũ. Trạng thái thật của kết nối đến từ nút thử, không từ lệnh lưu.
    if let Err(err) = harness.apply_provider().await {
        tracing::warn!("đã lưu nhưng chưa dùng được nhà cung cấp: {err}");
    }
    Ok(view(&saved))
}

#[tauri::command]
pub async fn remove_provider(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    harness
        .providers
        .remove(&id)
        .await
        .map_err(|err| err.to_string())?;
    // Xoá cái đang hoạt động thì kho tự chuyển sang cái khác; áp lại để `Driver` và bộ
    // nhúng đi theo, thay vì tiếp tục nói chuyện với một provider vừa bị xoá.
    if let Err(err) = harness.apply_provider().await {
        tracing::warn!("đã xoá nhưng chưa chuyển được sang nhà cung cấp khác: {err}");
    }
    Ok(())
}

#[tauri::command]
pub async fn set_active_provider(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    // `None` giữ nguyên mô hình đã chọn của provider ấy. Đổi provider không được lặng lẽ
    // đổi cả mô hình — người dùng chọn hai thứ đó riêng, và họ chỉ vừa đổi một.
    harness
        .providers
        .activate(&id, None)
        .await
        .map_err(|err| err.to_string())?;
    harness.apply_provider().await
}

#[tauri::command]
pub async fn set_provider_model(
    id: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let harness = state.harness().await?;
    harness
        .providers
        .activate(&id, Some(&model))
        .await
        .map_err(|err| err.to_string())?;
    harness.apply_provider().await
}

/// Thử một cấu hình **chưa lưu**.
///
/// Nhận cả cấu hình chứ không nhận một id, vì đây đúng là lúc người dùng chưa lưu: bắt họ
/// lưu trước rồi mới thử được nghĩa là một cấu hình sai vẫn phải nằm trong kho một lúc.
/// `api_key: None` thì mượn khoá đã lưu của cùng id — nếu không, thử một provider đã có
/// khoá sẽ luôn báo sai khoá.
#[tauri::command]
pub async fn probe_provider(
    input: ProviderInputWire,
    state: State<'_, AppState>,
) -> Result<ProviderProbe, String> {
    let harness = state.harness().await?;
    let wire = input;
    let mut config = ProviderConfig::new(
        wire.id.clone().unwrap_or_else(|| "thử".to_string()),
        wire.name.clone(),
        kind(&wire.kind)?,
        wire.base_url.clone(),
    );
    match (&wire.api_key, &wire.id) {
        (Some(key), _) => config = config.with_api_key(key.clone()),
        (None, Some(id)) => {
            let stored = harness
                .providers
                .store()
                .list()
                .map_err(|err| err.to_string())?
                .into_iter()
                .find(|item| item.id() == id);
            if let Some(stored) = stored {
                config = stored.config.clone();
                config.base_url = wire.base_url.clone();
                config.kind = kind(&wire.kind)?;
            }
        }
        (None, None) => {}
    }

    let result = harness.providers.probe(&config).await;
    Ok(ProviderProbe {
        ok: result.ok,
        message: result.message,
        models: result
            .models
            .into_iter()
            .map(|model| ModelChoice {
                id: model.id,
                tools: model.tools,
                context_window: model.context_window,
            })
            .collect(),
    })
}
