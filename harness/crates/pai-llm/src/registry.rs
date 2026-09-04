//! Configured providers, plus a cache of the adapters built from them.
//! No SQLite here: the crate takes an already-loaded `ProviderConfig`, because storage is
//! another layer's job. The cache is keyed by signature, not id, so an edited URL rebuilds.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::error::{LlmError, LlmErrorCode};
use crate::lmstudio::LmStudioAdapter;
use crate::ollama::OllamaAdapter;
use crate::openai::OpenAiAdapter;
use crate::seam::{LlmAdapter, ModelAdmin};

/// Server kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Ollama,
    /// LM Studio: OpenAI protocol for chat, but its own model store at `/api/v0` - see [`crate::lmstudio`].
    LmStudio,
    /// Anything speaking the OpenAI protocol: llama.cpp, vLLM, real OpenAI.
    OpenAiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
            Self::OpenAiCompatible => "openai",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "ollama" => Some(Self::Ollama),
            "lmstudio" | "lm_studio" | "lm-studio" | "lm studio" => Some(Self::LmStudio),
            "openai" | "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

/// Everything a built adapter depends on; `Debug` is hand-written so it never prints the API key, which would leak it into error logs.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProviderSignature {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    api_key: String,
}

impl fmt::Debug for ProviderSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderSignature")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "<trống>"
                } else {
                    "<đã đặt>"
                },
            )
            .finish()
    }
}

/// One configured server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub enabled: bool,
}

impl ProviderConfig {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: ProviderKind,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            base_url: base_url.into(),
            api_key: String::new(),
            enabled: true,
        }
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn signature(&self) -> ProviderSignature {
        ProviderSignature {
            id: self.id.clone(),
            kind: self.kind,
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
        }
    }

    /// True when the endpoint never leaves the loopback interface; the UI uses it to tell the user the data stays put.
    pub fn on_device(&self) -> bool {
        const LOOPBACK: [&str; 5] = ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"];
        let without_scheme = self
            .base_url
            .split("://")
            .nth(1)
            .unwrap_or(self.base_url.as_str());
        let authority = without_scheme.split('/').next().unwrap_or_default();
        // Strip the port, but leave `[::1]`, which has colons inside the brackets.
        let host = if let Some(rest) = authority.strip_prefix('[') {
            rest.split(']')
                .next()
                .map(|h| format!("[{h}]"))
                .unwrap_or_default()
        } else {
            authority.split(':').next().unwrap_or_default().to_string()
        };
        LOOPBACK.contains(&host.to_lowercase().as_str())
    }
}

/// The currently selected provider; three fallbacks in order - the pinned one, the first enabled one, then the first at all, because a disabled provider still beats "nothing configured".
pub fn active_config<'a>(
    configs: &'a [ProviderConfig],
    selected_id: &str,
) -> Option<&'a ProviderConfig> {
    if configs.is_empty() {
        return None;
    }
    configs
        .iter()
        .find(|config| config.id == selected_id.trim() && config.enabled)
        .or_else(|| configs.iter().find(|config| config.enabled))
        .or_else(|| configs.first())
}

/// Builds adapters and keeps them; the cache holds `reqwest`'s connection pool, so a fresh adapter per turn would mean a fresh TLS handshake per turn.
pub struct AdapterRegistry {
    http: reqwest::Client,
    cache: Mutex<HashMap<ProviderSignature, Arc<dyn LlmAdapter>>>,
}

impl AdapterRegistry {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The adapter for a config, built if absent.
    pub fn adapter(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmAdapter>, LlmError> {
        let signature = config.signature();
        {
            let cache = self.lock();
            if let Some(adapter) = cache.get(&signature) {
                return Ok(adapter.clone());
            }
        }
        let adapter: Arc<dyn LlmAdapter> = match config.kind {
            ProviderKind::Ollama => Arc::new(OllamaAdapter::new(
                config.id.clone(),
                &config.base_url,
                self.http.clone(),
            )),
            ProviderKind::LmStudio => Arc::new(LmStudioAdapter::new(
                config.id.clone(),
                &config.base_url,
                config.api_key.clone(),
                self.http.clone(),
            )),
            ProviderKind::OpenAiCompatible => Arc::new(OpenAiAdapter::new(
                config.id.clone(),
                &config.base_url,
                config.api_key.clone(),
                self.http.clone(),
            )?),
        };
        let mut cache = self.lock();
        // Drop everything built from an older shape of *this* provider; others keep their entries.
        cache.retain(|key, _| key.id != signature.id);
        cache.insert(signature, adapter.clone());
        Ok(adapter)
    }

    /// The lifecycle half of a provider; a remote provider answers with a sentence saying why it does not apply rather than a silent `None`.
    pub fn admin(&self, config: &ProviderConfig) -> Result<Arc<dyn ModelAdmin>, LlmError> {
        self.adapter(config)?.admin().ok_or_else(|| {
            LlmError::read_only(format!(
                "{} lưu mô hình từ xa; thao tác này không áp dụng",
                config.name
            ))
        })
    }

    /// How many adapters are held. For tests and `--dump-config`.
    pub fn cached(&self) -> usize {
        self.lock().len()
    }

    /// A poisoned lock only means another thread panicked while holding it; the `HashMap` is still consistent, so recover instead of panicking again.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ProviderSignature, Arc<dyn LlmAdapter>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// No providers left. A function so the message exists in exactly one place.
pub fn no_provider() -> LlmError {
    LlmError::new(
        LlmErrorCode::NoProviderConfigured,
        "Chưa cấu hình nhà cung cấp AI nào",
    )
}
