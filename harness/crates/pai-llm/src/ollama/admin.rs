//! Ollama model lifecycle: `/api/tags`, `/api/ps`, `/api/show`, `/api/pull`, `/api/delete`.
//! No client library models this half, which is why the crate uses `reqwest` directly
//! rather than borrowing an OpenAI client.

use std::time::Duration;

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::{BoxStream, StreamExt};
use serde_json::{Value, json};

use crate::capabilities::{
    Capabilities, context_length_from_model_info, normalize_ollama_capabilities,
};
use crate::error::LlmError;
use crate::model::{ModelDetails, ModelInfo, ModelState, PullProgress, RunningModel};
use crate::seam::ModelAdmin;
use crate::wire::LineDecoder;

/// How long before a health probe declares the server dead.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub struct OllamaAdmin {
    base_url: String,
    http: reqwest::Client,
    /// A separate, untimed client for `/api/pull`: a ten-gigabyte download outlives any sane request budget, and dropping the stream cancels it.
    pull_http: reqwest::Client,
}

impl OllamaAdmin {
    pub fn new(base_url: impl Into<String>, http: reqwest::Client) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        // `build()` only fails when the OS cannot set up TLS; reusing the existing client beats falling over.
        let pull_http = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| http.clone());
        Self {
            base_url,
            http,
            pull_http,
        }
    }

    pub async fn health(&self) -> bool {
        self.http
            .get(format!("{}/api/ps", self.base_url))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    async fn get(&self, path: &str) -> Result<Value, LlmError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .send()
            .await?;
        read_json(response).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, LlmError> {
        let response = self
            .http
            .post(format!("{}{path}", self.base_url))
            .json(&body)
            .send()
            .await?;
        read_json(response).await
    }

    /// Raw `/api/show` for one model.
    async fn show_raw(&self, model: &str) -> Result<Value, LlmError> {
        self.post("/api/show", json!({ "model": model, "verbose": false }))
            .await
    }
}

async fn read_json(response: reqwest::Response) -> Result<Value, LlmError> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(LlmError::from_status(status.as_u16(), &body));
    }
    let text = response.text().await?;
    serde_json::from_str(&text).map_err(LlmError::from)
}

/// Read capabilities from an `/api/show` response, falling back to name inference when the server is silent.
fn details_from_show(model: &str, payload: &Value) -> ModelDetails {
    let context_window = payload
        .get("model_info")
        .and_then(Value::as_object)
        .and_then(context_length_from_model_info);
    let reported = payload
        .get("capabilities")
        .map(normalize_ollama_capabilities)
        .unwrap_or_default();
    let details = payload.get("details");
    let family = details
        .and_then(|d| d.get("family"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let capabilities =
        Capabilities::from_reported(&reported, context_window).unwrap_or_else(|| {
            // Older Ollama has no `capabilities` field. Guess from name *plus* family, as the Python side did.
            let mut inferred =
                Capabilities::infer(&format!("{model} {}", family.clone().unwrap_or_default()));
            inferred.context_window = context_window;
            inferred
        });
    ModelDetails {
        capabilities,
        family,
        parameter_size: details
            .and_then(|d| d.get("parameter_size"))
            .and_then(Value::as_str)
            .map(str::to_string),
        quantization: details
            .and_then(|d| d.get("quantization_level"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

#[async_trait]
impl ModelAdmin for OllamaAdmin {
    async fn list(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let installed = self.get("/api/tags").await?;
        let running = self.get("/api/ps").await?;
        let installed = installed
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let running = running
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut models = Vec::new();
        for item in &installed {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if name.is_empty() {
                continue;
            }
            let active = running.iter().find(|entry| {
                entry
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|other| other == name)
            });
            // One POST `/api/show` per model. Expensive, but the only authoritative source: name guessing is wrongest for self-named fine-tunes.
            let details = match self.show_raw(&name).await {
                Ok(payload) => details_from_show(&name, &payload),
                Err(err) => {
                    tracing::debug!(%err, model = %name, "/api/show did not answer, guessing from the name");
                    let family = item
                        .get("details")
                        .and_then(|d| d.get("family"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    ModelDetails {
                        capabilities: Capabilities::infer(&format!("{name} {family}")),
                        family: (!family.is_empty()).then(|| family.to_string()),
                        parameter_size: None,
                        quantization: None,
                    }
                }
            };
            models.push(ModelInfo {
                name,
                state: if active.is_some() {
                    ModelState::Loaded
                } else {
                    ModelState::Unloaded
                },
                size_bytes: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                vram_bytes: active
                    .and_then(|entry| entry.get("size_vram"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                quantization: details.quantization.clone().or_else(|| {
                    item.get("details")
                        .and_then(|d| d.get("quantization_level"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }),
                modified_at: item
                    .get("modified_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                capabilities: details.capabilities,
            });
        }
        Ok(models)
    }

    async fn running(&self) -> Result<Vec<RunningModel>, LlmError> {
        let payload = self.get("/api/ps").await?;
        let models = payload
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(models
            .iter()
            .filter_map(|item| {
                let name = item.get("name").and_then(Value::as_str)?.to_string();
                Some(RunningModel {
                    name,
                    size_bytes: item.get("size").and_then(Value::as_u64).unwrap_or(0),
                    vram_bytes: item.get("size_vram").and_then(Value::as_u64).unwrap_or(0),
                    expires_at: item
                        .get("expires_at")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
            })
            .collect())
    }

    async fn show(&self, model: &str) -> Result<ModelDetails, LlmError> {
        let payload = self.show_raw(model).await?;
        Ok(details_from_show(model, &payload))
    }

    fn pull(&self, model: &str) -> BoxStream<'_, Result<PullProgress, LlmError>> {
        let http = self.pull_http.clone();
        let url = format!("{}/api/pull", self.base_url);
        let body = json!({ "model": model, "stream": true });
        let request = async move {
            http.post(url)
                .json(&body)
                .send()
                .await
                .map_err(LlmError::from)
        }
        .boxed();

        // Small state machine: send, read bytes, split lines, decode. Not `pump`, which speaks `StreamChunk` while the unit here is download progress.
        enum State {
            Connecting(futures::future::BoxFuture<'static, Result<reqwest::Response, LlmError>>),
            Reading {
                body: BoxStream<'static, Result<Vec<u8>, LlmError>>,
                lines: LineDecoder,
                queue: std::collections::VecDeque<PullProgress>,
            },
            Done,
        }

        futures::stream::unfold(State::Connecting(request), |state| async move {
            let mut state = state;
            loop {
                match state {
                    State::Connecting(request) => match request.await {
                        Err(err) => return Some((Err(err), State::Done)),
                        Ok(response) => {
                            let status = response.status();
                            if !status.is_success() {
                                let text = response.text().await.unwrap_or_default();
                                return Some((
                                    Err(LlmError::from_status(status.as_u16(), &text)),
                                    State::Done,
                                ));
                            }
                            let body = response
                                .bytes_stream()
                                .map(|item| {
                                    item.map(|bytes| bytes.to_vec()).map_err(LlmError::from)
                                })
                                .boxed();
                            state = State::Reading {
                                body,
                                lines: LineDecoder::new(),
                                queue: std::collections::VecDeque::new(),
                            };
                        }
                    },
                    State::Reading { body, lines, queue } => {
                        let mut body = body;
                        let mut lines = lines;
                        let mut queue = queue;
                        if let Some(progress) = queue.pop_front() {
                            return Some((Ok(progress), State::Reading { body, lines, queue }));
                        }
                        match body.next().await {
                            Some(Ok(bytes)) => {
                                for line in lines.push(&bytes) {
                                    match decode_pull_line(&line) {
                                        Ok(Some(progress)) => queue.push_back(progress),
                                        Ok(None) => {}
                                        Err(err) => return Some((Err(err), State::Done)),
                                    }
                                }
                                state = State::Reading { body, lines, queue };
                            }
                            Some(Err(err)) => return Some((Err(err), State::Done)),
                            None => {
                                if let Some(rest) = lines.flush()
                                    && let Ok(Some(progress)) = decode_pull_line(&rest)
                                {
                                    return Some((Ok(progress), State::Done));
                                }
                                return None;
                            }
                        }
                    }
                    State::Done => return None,
                }
            }
        })
        .boxed()
    }

    async fn unload(&self, model: &str) -> Result<(), LlmError> {
        // Ollama has no "unload" verb; `keep_alive: 0` on `/api/generate` is the official way, not a trick.
        self.post("/api/generate", json!({ "model": model, "keep_alive": 0 }))
            .await?;
        Ok(())
    }

    async fn delete(&self, model: &str) -> Result<(), LlmError> {
        let response = self
            .http
            .delete(format!("{}/api/delete", self.base_url))
            .json(&json!({ "model": model }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status.as_u16(), &body));
        }
        Ok(())
    }
}

fn decode_pull_line(line: &str) -> Result<Option<PullProgress>, LlmError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        tracing::warn!(line = trimmed, "unreadable progress line, skipping");
        return Ok(None);
    };
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return Err(LlmError::unavailable(message.to_string()));
    }
    Ok(Some(PullProgress {
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        digest: value
            .get("digest")
            .and_then(Value::as_str)
            .map(str::to_string),
        total: value.get("total").and_then(Value::as_u64),
        completed: value.get("completed").and_then(Value::as_u64),
    }))
}
