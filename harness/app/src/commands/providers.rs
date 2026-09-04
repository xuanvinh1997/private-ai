//! Model providers: listing, editing, probing, and switching the active one.

use pai_llm::{ProviderConfig, ProviderKind};
use pai_providers::{ProviderInput, StoredProvider};
use tauri::State;

use crate::AppState;
use crate::harness::Harness;
use crate::protocol::{
    EmbeddingProbe, EmbeddingSetting, ModelChoice, ProviderInputWire, ProviderPreset,
    ProviderProbe, ProviderView, VisionProbe, VisionSetting,
};

/// Wire string to provider kind; rejected rather than defaulted, since a mistyped `kind` silently becoming
/// `openai` would send the wrong protocol to Ollama and surface as a JSON error.
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
        active_chat: stored.active_chat,
        active_embedding: stored.active_embedding,
        active_vision: stored.active_vision,
        model: stored.model.clone(),
        embedding_model: stored.embedding_model.clone(),
        vision_model: stored.vision_model.clone(),
    }
}

fn input(wire: ProviderInputWire) -> Result<ProviderInput, String> {
    let mut built = ProviderInput::create(wire.name, kind(&wire.kind)?, wire.base_url);
    built.id = wire.id;
    built.enabled = wire.enabled;
    built.model = wire.model;
    built.api_key = wire.api_key;
    built.embedding_model = wire.embedding_model;
    built.vision_model = wire.vision_model;
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
    // No harness needed, but `state` is still taken so every command in this group has the same shape.
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

/// Save a provider, then reapply the active one -- even when the saved row is not active, since editing the
/// running provider's URL without reapplying keeps sending requests to the old server, silently.
#[tauri::command]
pub async fn save_provider(
    input: ProviderInputWire,
    state: State<'_, AppState>,
) -> Result<ProviderView, String> {
    let harness = state.harness().await?;
    let vision_model = input.vision_model.clone();
    let mut saved = harness
        .providers
        .save(self::input(input)?)
        .await
        .map_err(|err| err.to_string())?;
    // The first explicitly configured vision model becomes the OCR provider. Editing that same provider keeps
    // it active; configuring a second provider does not silently move image data to a different endpoint.
    if let Some(model) = vision_model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
    {
        let no_holder = harness
            .providers
            .vision()
            .map_err(|err| err.to_string())?
            .is_none();
        if no_holder || saved.active_vision {
            saved = harness
                .providers
                .set_vision(saved.id(), Some(model))
                .await
                .map_err(|err| err.to_string())?;
        }
    }
    // Reapplying can fail, but the row is already saved, and erroring here would make the UI revert the form.
    // Real connection state comes from the probe button, not from saving.
    if let Err(err) = harness.apply_provider().await {
        tracing::warn!("saved, but could not activate the provider: {err}");
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
    // Deleting the active one makes the store elect a successor; reapply so `Driver` and the embedder follow.
    if let Err(err) = harness.apply_provider().await {
        tracing::warn!("removed, but could not switch to another provider: {err}");
    }
    Ok(())
}

#[tauri::command]
pub async fn set_active_provider(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    // `None` keeps that provider's chosen model: switching providers must not silently switch models too.
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
    apply_chat_model(&harness, &id, &model).await
}

/// The effective model in the shared driver. Reading the driver rather than the stored row also covers a
/// provider whose model is still inherited from its built-in preset.
#[tauri::command]
pub async fn active_chat_model(state: State<'_, AppState>) -> Result<String, String> {
    let harness = state.harness().await?;
    Ok(harness.driver.model())
}

/// Pick the model of the provider that currently holds the chat role. The composer deliberately does not know
/// provider ids: its catalogue and this write must both follow whichever provider the core considers active.
#[tauri::command]
pub async fn set_active_chat_model(
    model: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let harness = state.harness().await?;
    let active = harness
        .providers
        .active()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| pai_llm::registry::no_provider().to_string())?;
    apply_chat_model(&harness, active.id(), &model).await?;
    Ok(harness.driver.model())
}

async fn apply_chat_model(harness: &Harness, id: &str, model: &str) -> Result<(), String> {
    harness
        .providers
        .activate(id, Some(model))
        .await
        .map_err(|err| err.to_string())?;
    harness.apply_provider().await
}

/// Probe an unsaved configuration, taking the whole config rather than an id; `api_key: None` borrows the
/// stored key for the same id, or probing a saved provider would always report a bad key.
/// The returned capabilities carry two confidence levels: LM Studio declares model types, while Ollama and
/// OpenAI-compatible are inferred from names, so never warn about `tools` from a probe.
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
                chat: model.chat,
                embedding: model.embedding,
                vision: model.vision,
                context_window: model.context_window,
            })
            .collect(),
    })
}

/// Model catalogue for any saved provider. Separate from `list_models`, which only asks the chat-role
/// provider, while the embedding provider is usually a different server. An empty list means "could not ask",
/// not an error, so the UI must still allow manual entry.
#[tauri::command]
pub async fn provider_models(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelChoice>, String> {
    let harness = state.harness().await?;
    let Some(provider) = harness
        .providers
        .list()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|item| item.id() == provider_id)
    else {
        return Ok(Vec::new());
    };

    Ok(harness
        .providers
        .models(&provider.config)
        .await
        .into_iter()
        .map(|model| ModelChoice {
            id: model.id,
            tools: model.tools,
            chat: model.chat,
            embedding: model.embedding,
            vision: model.vision,
            context_window: model.context_window,
        })
        .collect())
}

/// The embedding configuration currently in effect, assembled here rather than filtered by the UI, because
/// "what embeds my documents, and does it work" is one question.
#[tauri::command]
pub async fn embedding_setting(state: State<'_, AppState>) -> Result<EmbeddingSetting, String> {
    let harness = state.harness().await?;
    let held = harness
        .providers
        .embedding()
        .map_err(|err| err.to_string())?;
    let reason = pai_providers::embedding_reason(held.as_ref());
    Ok(match held {
        Some(provider) => EmbeddingSetting {
            provider_name: Some(provider.config.name.clone()),
            on_device: provider.config.on_device(),
            provider_id: Some(provider.id().to_string()),
            model: provider.embedding_model.clone(),
            reason,
        },
        None => EmbeddingSetting {
            provider_id: None,
            provider_name: None,
            model: None,
            on_device: false,
            reason,
        },
    })
}

/// Grant the embedding role to a provider with an explicit model; there is no "let the core pick" branch,
/// since a wrong embedding model fails every ingest and an implicit choice is one nobody remembers making.
#[tauri::command]
pub async fn set_embedding(
    provider_id: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<EmbeddingSetting, String> {
    let harness = state.harness().await?;
    harness
        .providers
        .set_embedding(&provider_id, Some(&model))
        .await
        .map_err(|err| err.to_string())?;
    harness.apply_provider().await?;
    embedding_setting(state).await
}

/// Really embed a sentence using an unsaved configuration.
#[tauri::command]
pub async fn probe_embedding(
    provider_id: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<EmbeddingProbe, String> {
    let harness = state.harness().await?;
    let provider = harness
        .providers
        .list()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|item| item.id() == provider_id)
        .ok_or_else(|| format!("không có nhà cung cấp `{provider_id}`"))?;

    let result = harness
        .providers
        .probe_embedding(&provider.config, &model)
        .await;
    Ok(EmbeddingProbe {
        ok: result.ok,
        message: result.message,
        dimensions: result.dimensions,
    })
}

/// The vision configuration in effect. The OCR switch lives in the RAG config file rather than the provider
/// store, and is read here too: "will a scanned page be read" is one question, not two screens.
#[tauri::command]
pub async fn vision_setting(state: State<'_, AppState>) -> Result<VisionSetting, String> {
    let harness = state.harness().await?;
    let held = harness.providers.vision().map_err(|err| err.to_string())?;
    let reason = pai_providers::vision_reason(held.as_ref());
    let (ocr_enabled, _) = harness.rag_config.ocr_status();
    let ocr_images = harness.rag_config.ocr_images();
    Ok(match held {
        Some(provider) => VisionSetting {
            provider_name: Some(provider.config.name.clone()),
            on_device: provider.config.on_device(),
            provider_id: Some(provider.id().to_string()),
            model: provider.vision_model.clone(),
            reason,
            ocr_enabled,
            ocr_images,
        },
        None => VisionSetting {
            provider_id: None,
            provider_name: None,
            model: None,
            on_device: false,
            reason,
            ocr_enabled,
            ocr_images,
        },
    })
}

/// Grant the vision role to a provider with an explicit model. Like [`set_embedding`] there is no implicit
/// branch: a chat model that cannot see returns a 400 per page, hours after the choice was made.
#[tauri::command]
pub async fn set_vision(
    provider_id: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<VisionSetting, String> {
    let harness = state.harness().await?;
    harness
        .providers
        .set_vision(&provider_id, Some(&model))
        .await
        .map_err(|err| err.to_string())?;
    // Rewrites `rag-config.json`, which is where the document library reads the vision provider from.
    harness.apply_provider().await?;
    vision_setting(state).await
}

/// Really read a bundled test image with an unsaved model name; a reachable model list proves nothing about
/// whether that model can see.
#[tauri::command]
pub async fn probe_vision(
    provider_id: String,
    model: String,
    state: State<'_, AppState>,
) -> Result<VisionProbe, String> {
    let harness = state.harness().await?;
    let provider = harness
        .providers
        .list()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|item| item.id() == provider_id)
        .ok_or_else(|| format!("không có nhà cung cấp `{provider_id}`"))?;

    let result = pai_providers::probe_vision(&provider.config, &model).await;
    Ok(VisionProbe {
        ok: result.ok,
        message: result.message,
        text: result.text,
    })
}

/// The optional half of OCR: reading pictures inside pages that already have text. Separate from
/// [`crate::commands::docs::set_ocr_enabled`] because they answer different questions -- "read scans at all"
/// versus "also read the illustrations" -- and a document full of photos should not cost a request each.
#[tauri::command]
pub async fn set_ocr_images(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<VisionSetting, String> {
    let harness = state.harness().await?;
    harness
        .rag_config
        .write_ocr_images(enabled)
        .map_err(|error| format!("không lưu được cấu hình đọc ảnh: {error}"))?;
    vision_setting(state).await
}
