//! The single path for switching providers. A swap touches three things -- the stored row, the adapter
//! cache, and the `Driver` holding the adapter -- and any caller doing it by hand will eventually skip
//! the third, giving "I switched but it still uses the old server" with nothing in the log.

use std::sync::Arc;

use pai_agent::Driver;
use pai_llm::{AdapterRegistry, ProviderConfig};

use crate::error::{ProviderError, Result};
use crate::presets;
use crate::probe::{EmbeddingProbeResult, ProbeResult, probe, probe_embedding};
use crate::store::{ProviderInput, ProviderStore, Role, StoredProvider};

/// A model a provider offers, with its capabilities; distinct from [`crate::probe::ProbeModel`], which is
/// only what a server claimed during a connection test.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelListing {
    pub id: String,
    pub chat: bool,
    /// Embedding-capable; the field the embedding screen reads so it need not guess a default name.
    pub embedding: bool,
    pub tools: bool,
    pub context_window: Option<u64>,
}

pub struct ProviderRuntime {
    store: Arc<dyn ProviderStore>,
    registry: Arc<AdapterRegistry>,
    driver: Arc<Driver>,
    http: reqwest::Client,
}

impl ProviderRuntime {
    pub fn new(
        store: Arc<dyn ProviderStore>,
        registry: Arc<AdapterRegistry>,
        driver: Arc<Driver>,
        http: reqwest::Client,
    ) -> ProviderRuntime {
        ProviderRuntime {
            store,
            registry,
            driver,
            http,
        }
    }

    pub fn store(&self) -> &dyn ProviderStore {
        self.store.as_ref()
    }

    pub fn registry(&self) -> &AdapterRegistry {
        &self.registry
    }

    pub fn list(&self) -> Result<Vec<StoredProvider>> {
        self.store.list()
    }

    /// The provider holding the chat role.
    pub fn active(&self) -> Result<Option<StoredProvider>> {
        self.store.active(Role::Chat)
    }

    /// The provider holding the embedding role; `None` is common and valid, because nobody has been asked yet.
    pub fn embedding(&self) -> Result<Option<StoredProvider>> {
        self.store.active(Role::Embedding)
    }

    pub fn vision(&self) -> Result<Option<StoredProvider>> {
        self.store.active(Role::Vision)
    }

    /// Hand over the embedding role, with a model if one was picked; it never touches `Driver`, but still goes
    /// through [`ProviderRuntime::resync`], since one reapply path is the whole point of this runtime.
    pub async fn set_embedding(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        let active = self.store.activate(Role::Embedding, id, model)?;
        self.resync().await;
        Ok(active)
    }

    pub async fn set_vision(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        let active = self.store.activate(Role::Vision, id, model)?;
        self.resync().await;
        Ok(active)
    }

    /// Save a form and resync, even when the edited row is not the active one: selection has three fallback
    /// tiers, so toggling `enabled` can change the winner.
    pub async fn save(&self, input: ProviderInput) -> Result<StoredProvider> {
        let saved = self.store.save(input)?;
        self.resync().await;
        Ok(saved)
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        self.store.remove(id)?;
        self.resync().await;
        Ok(())
    }

    /// Hand over the chat role.
    pub async fn activate(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        let active = self.store.activate(Role::Chat, id, model)?;
        // This is the switch the user actually asked for, so errors surface instead of sinking into a log line as in `resync`.
        self.apply_active().await?;
        Ok(active)
    }

    /// Build an adapter from the chat-role provider and push it into [`Driver`]; the embedding provider may be
    /// a different server entirely. `async` reserves room for network work here without resigning ten call sites.
    pub async fn apply_active(&self) -> Result<()> {
        let Some(active) = self.store.active(Role::Chat)? else {
            return Err(ProviderError::Llm(pai_llm::registry::no_provider()));
        };
        let adapter = self.registry.adapter(&active.config)?;
        self.driver.set_llm(adapter);
        if let Some(model) = model_for(&active) {
            self.driver.set_model(model);
        }
        tracing::info!(
            provider = %active.config.name,
            on_device = active.config.on_device(),
            model = %self.driver.model(),
            "switched active provider"
        );
        Ok(())
    }

    /// Probe an unsaved configuration.
    pub async fn probe(&self, config: &ProviderConfig) -> ProbeResult {
        probe(config, &self.http).await
    }

    /// Model catalogue for a saved provider, with per-model capabilities. Unlike `probe`, which only answers
    /// "can I connect", this answers "which models here can embed" -- unanswerable from a guessed name.
    /// Ask the server first, fall back to name inference; an empty list means "could not ask", so callers must
    /// keep a manual entry path. Saved configs only, since this goes through the registry's adapter cache.
    pub async fn models(&self, config: &ProviderConfig) -> Vec<ModelListing> {
        if let Ok(admin) = self.registry.admin(config) {
            match admin.list().await {
                Ok(models) => {
                    return models
                        .into_iter()
                        .map(|model| ModelListing {
                            id: model.name,
                            chat: model.capabilities.chat,
                            embedding: model.capabilities.embedding,
                            tools: model.capabilities.tools,
                            context_window: model.capabilities.context_window,
                        })
                        .collect();
                }
                // Fall through to listing rather than returning empty: a broken `/api/show` says nothing about `/api/tags`.
                Err(err) => tracing::warn!(
                    provider = %config.name,
                    "could not read the model catalogue, falling back to listing: {err}"
                ),
            }
        }

        probe(config, &self.http)
            .await
            .models
            .into_iter()
            // Keep every flag as `probe` computed it: a second copy of that rule here would drift from the first.
            .map(|model| ModelListing {
                id: model.id,
                chat: model.chat,
                embedding: model.embedding,
                tools: model.tools,
                context_window: model.context_window,
            })
            .collect()
    }

    /// Really embed a sentence with a model, on a possibly unsaved configuration.
    pub async fn probe_embedding(
        &self,
        config: &ProviderConfig,
        model: &str,
    ) -> EmbeddingProbeResult {
        probe_embedding(config, model).await
    }

    /// Resync after a change that already succeeded; errors become a log line, because deleting the last provider
    /// is valid and must succeed even though nothing is left to build an adapter from.
    async fn resync(&self) {
        if let Err(err) = self.apply_active().await {
            tracing::warn!("could not apply the active provider: {err}");
        }
    }
}

/// Which model for this provider: the user's choice, else the catalogue default for the same address.
/// `None` means keep the current model name -- inventing one guarantees failure, keeping it may still work.
fn model_for(provider: &StoredProvider) -> Option<String> {
    provider.model.clone().or_else(|| {
        presets::matching(&provider.config.base_url)
            .and_then(|preset| preset.default_model)
            .map(str::to_string)
    })
}
