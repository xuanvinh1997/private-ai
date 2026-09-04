//! LM Studio adapter: chat over `/v1/chat/completions`, model store over `/api/v0`.
//! It exists apart from the OpenAI adapter only for `/api/v0`, which reports real
//! capabilities and VRAM state; the chat half reuses OpenAI's wire plus `/v1` and `ttl`.

pub mod admin;

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::BoxStream;
use serde_json::{Value, json};

use crate::capabilities::Capabilities;
use crate::error::LlmError;
use crate::message::ChatRequest;
use crate::seam::{LlmAdapter, ModelAdmin};
use crate::stream::StreamChunk;
use crate::wire::pump::pump;

pub use admin::LmStudioAdmin;

/// Talks to an LM Studio server.
pub struct LmStudioAdapter {
    id: String,
    /// The *server root*, with no `/v1` or `/api/v0` suffix, since this adapter speaks both protocols on one host.
    base_url: String,
    api_key: String,
    http: reqwest::Client,
    admin: Arc<LmStudioAdmin>,
}

impl LmStudioAdapter {
    /// `base_url` accepts `http://localhost:1234`, `.../v1` and `.../api/v0`, because all three are what users actually have in hand.
    pub fn new(
        id: impl Into<String>,
        base_url: impl AsRef<str>,
        api_key: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        let base_url = server_root(base_url.as_ref());
        let api_key = api_key.into();
        let admin = Arc::new(LmStudioAdmin::new(
            base_url.clone(),
            api_key.clone(),
            http.clone(),
        ));
        Self {
            id: id.into(),
            base_url,
            api_key,
            http,
            admin,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            // A local LM Studio needs no key, and a fake `Bearer` header is just another meaningless string in the logs.
            return builder;
        }
        builder.bearer_auth(&self.api_key)
    }
}

/// Strip every known API suffix down to the server root; repeatedly, since `/api/v0` is two segments and pasted URLs can carry both.
pub fn server_root(base_url: &str) -> String {
    let mut value = base_url.trim().trim_end_matches('/');
    loop {
        let tail = value.rsplit('/').next().unwrap_or_default();
        let versioned = tail.starts_with('v')
            && tail.len() > 1
            && tail[1..].chars().all(|c| c.is_ascii_digit());
        if !(versioned || tail == "api") {
            break;
        }
        value = value[..value.len() - tail.len()].trim_end_matches('/');
    }
    value.to_string()
}

#[async_trait]
impl LlmAdapter for LmStudioAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        let body = encode_chat(&req);
        let builder = self.authorized(
            self.http
                .post(format!("{}/v1/chat/completions", self.base_url)),
        );
        let request =
            async move { builder.json(&body).send().await.map_err(LlmError::from) }.boxed();
        // The same SSE decoder as OpenAI, not a copy: LM Studio emits exactly that shape, `reasoning_content` included.
        pump(request, crate::openai::ChatDecoder::new())
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        // Same mandatory order as Ollama - ask the server, then guess by name. This is the whole reason the adapter is separate.
        Ok(self
            .admin
            .show(model)
            .await
            .map(|details| details.capabilities)
            .unwrap_or_else(|_| Capabilities::infer(model)))
    }

    async fn health(&self) -> bool {
        self.admin.health().await
    }

    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        Some(self.admin.clone())
    }
}

/// The `/v1/chat/completions` body plus LM Studio's `ttl`; built on [`crate::openai::encode_chat`] so later fixes there carry over.
pub(crate) fn encode_chat(req: &ChatRequest) -> Value {
    let mut body = crate::openai::encode_chat(req);
    // `keep_alive` is Ollama vocabulary; LM Studio calls it `ttl` and measures seconds, so translate rather than drop it.
    if let Some(seconds) = req.keep_alive.as_deref().and_then(keep_alive_seconds)
        && let Some(object) = body.as_object_mut()
    {
        object.insert("ttl".into(), json!(seconds));
    }
    body
}

/// `"5m"`, `"30s"`, `"1h"`, `"0"`, `"300"` -> seconds; `None` when unreadable, because a lifetime hint must never break the turn.
fn keep_alive_seconds(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (number, factor) = match value.chars().last()? {
        's' => (&value[..value.len() - 1], 1),
        'm' => (&value[..value.len() - 1], 60),
        'h' => (&value[..value.len() - 1], 3600),
        _ => (value, 1),
    };
    number.trim().parse::<u64>().ok().map(|n| n * factor)
}
