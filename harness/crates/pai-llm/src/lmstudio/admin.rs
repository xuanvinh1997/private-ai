//! LM Studio model store: `/api/v0/models` and `/api/v0/models/{id}`.
//! It answers what `/v1/models` cannot: what a model can do and whether it is in VRAM.
//! Pull/unload/delete return `Unsupported` with a fix, rather than dropping `list`/`show`.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use serde_json::Value;

use crate::capabilities::Capabilities;
use crate::error::{LlmError, LlmErrorCode};
use crate::model::{ModelDetails, ModelInfo, ModelState, PullProgress, RunningModel};
use crate::seam::ModelAdmin;

/// How long before a health probe declares the server dead.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub struct LmStudioAdmin {
    /// Server root with every API suffix stripped - see [`super::server_root`].
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl LmStudioAdmin {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            http,
        }
    }

    pub async fn health(&self) -> bool {
        self.authorized(self.http.get(format!("{}/api/v0/models", self.base_url)))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            return builder;
        }
        builder.bearer_auth(&self.api_key)
    }

    async fn get(&self, path: &str) -> Result<Value, LlmError> {
        let response = self
            .authorized(self.http.get(format!("{}{path}", self.base_url)))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status.as_u16(), &body));
        }
        let text = response.text().await?;
        serde_json::from_str(&text).map_err(LlmError::from)
    }

    /// One shared explanation for the three verbs that do not exist here.
    fn khong_co(verb: &str, thay_the: &str) -> LlmError {
        LlmError::new(
            LlmErrorCode::Unsupported,
            format!("LM Studio không có API để {verb} qua máy chủ cục bộ. {thay_the}"),
        )
    }
}

/// Read one `/api/v0/models` entry into capabilities, from three sources in falling order of trust - the `capabilities` array, the loose boolean flags, then `type` - and fall back to name plus `arch` when none answers.
pub fn details_from_model(name: &str, entry: &Value) -> ModelDetails {
    let context_window = entry
        .get("loaded_context_length")
        .and_then(Value::as_u64)
        .or_else(|| entry.get("max_context_length").and_then(Value::as_u64));
    let arch = entry
        .get("arch")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut reported: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.to_string();
        if !reported.contains(&name) {
            reported.push(name);
        }
    };

    // (1) LM Studio's own vocabulary, translated into this crate's.
    if let Some(items) = entry.get("capabilities").and_then(Value::as_array) {
        for item in items.iter().filter_map(Value::as_str) {
            match item.to_lowercase().as_str() {
                "tool_use" | "tools" | "function_calling" => push("tools"),
                "vision" | "image_input" => push("vision"),
                "embedding" | "embeddings" => push("embedding"),
                "thinking" | "reasoning" => push("thinking"),
                "chat" | "completion" => push("chat"),
                _ => {}
            }
        }
    }
    // (2) Loose flags, read only when `true`: a `false` here is usually an unfilled field, not a denial.
    if entry
        .get("trained_for_tool_use")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        push("tools");
    }
    if entry
        .get("vision")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        push("vision");
    }
    // (3) Model type.
    match entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "embeddings" | "embedding" => push("embedding"),
        "vlm" => {
            push("chat");
            push("vision");
        }
        "llm" => push("chat"),
        _ => {}
    }

    let capabilities =
        Capabilities::from_reported(&reported, context_window).unwrap_or_else(|| {
            let mut inferred =
                Capabilities::infer(&format!("{name} {}", arch.clone().unwrap_or_default()));
            inferred.context_window = context_window;
            inferred
        });

    ModelDetails {
        capabilities,
        family: arch,
        // LM Studio publishes no parameter count anywhere; it lives in the filename and we do not guess.
        parameter_size: None,
        quantization: entry
            .get("quantization")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Is this entry currently in VRAM?
fn loaded(entry: &Value) -> bool {
    entry
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| state.eq_ignore_ascii_case("loaded"))
}

fn info_from_entry(entry: &Value) -> Option<ModelInfo> {
    let name = entry.get("id").and_then(Value::as_str)?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let details = details_from_model(&name, entry);
    Some(ModelInfo {
        state: if loaded(entry) {
            ModelState::Loaded
        } else {
            ModelState::Unloaded
        },
        // LM Studio reports neither file size nor VRAM use; 0 means unknown, and inventing a number would feed `required_bytes`.
        size_bytes: 0,
        vram_bytes: 0,
        quantization: details.quantization.clone(),
        modified_at: None,
        capabilities: details.capabilities,
        name,
    })
}

#[async_trait]
impl ModelAdmin for LmStudioAdmin {
    async fn list(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let payload = self.get("/api/v0/models").await?;
        let Some(entries) = payload.get("data").and_then(Value::as_array) else {
            return Err(LlmError::invalid(
                "LM Studio trả về danh sách mô hình không hợp lệ",
            ));
        };
        // One call for the whole store, unlike Ollama: `/api/v0/models` already carries `type`, `state` and the context window per entry.
        let mut models: Vec<ModelInfo> = entries.iter().filter_map(info_from_entry).collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }

    async fn running(&self) -> Result<Vec<RunningModel>, LlmError> {
        let payload = self.get("/api/v0/models").await?;
        let entries = payload
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(entries
            .iter()
            .filter(|entry| loaded(entry))
            .filter_map(|entry| {
                Some(RunningModel {
                    name: entry.get("id").and_then(Value::as_str)?.to_string(),
                    size_bytes: 0,
                    vram_bytes: 0,
                    // LM Studio expires per-request via `ttl`, not per model, so there is no deadline to print.
                    expires_at: None,
                })
            })
            .collect())
    }

    async fn show(&self, model: &str) -> Result<ModelDetails, LlmError> {
        // LM Studio model names contain `/`, so the segment must be encoded or the path grows a component and the server 404s.
        let payload = self
            .get(&format!("/api/v0/models/{}", encode_segment(model)))
            .await?;
        Ok(details_from_model(model, &payload))
    }

    fn pull(&self, model: &str) -> BoxStream<'_, Result<PullProgress, LlmError>> {
        let err = Self::khong_co(
            "tải mô hình về",
            &format!(
                "Tải `{model}` trong chính ứng dụng LM Studio (tab Discover), hoặc bằng \
                 `lms get {model}`, rồi bấm làm mới ở đây."
            ),
        );
        Box::pin(stream::once(async move { Err(err) }))
    }

    async fn unload(&self, model: &str) -> Result<(), LlmError> {
        Err(Self::khong_co(
            "nhả mô hình khỏi VRAM",
            &format!(
                "LM Studio nhả theo bộ đếm `ttl` của chính nó; đặt thời gian giữ ấm trong \
                 tab Developer, hoặc chạy `lms unload {model}`."
            ),
        ))
    }

    async fn delete(&self, model: &str) -> Result<(), LlmError> {
        Err(Self::khong_co(
            "xoá mô hình khỏi đĩa",
            &format!("Xoá `{model}` trong tab My Models của LM Studio."),
        ))
    }
}

/// Percent-encode one path segment; hand-written, because a URL crate for a single function is a dependency to maintain forever.
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
