//! Model-layer seam: two replaceable capabilities, declared as marker types.
//! [`Llm`] is talking to a model, which every provider does; [`Models`] is local model
//! lifecycle, which only Ollama has, so a remote provider says "n/a" at the type level.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_core::ServiceKey;

use crate::capabilities::Capabilities;
use crate::error::LlmError;
use crate::message::ChatRequest;
use crate::model::{ModelDetails, ModelInfo, PullProgress, RunningModel};
use crate::stream::StreamChunk;

/// Talk to a model; `stream` is deliberately not an `async fn`, because `#[async_trait]` would box a future around an already-boxed stream and force an `.await` before the first chunk.
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// Id of the provider this adapter serves. Appears in logs and error messages.
    fn id(&self) -> &str;

    /// Chunk stream for one request; it ends with exactly one [`StreamChunk::Finish`] or an `Err`, and cancelling means dropping the stream.
    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>>;

    /// What this model can do. Implementations must ask the server first and guess by name second.
    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError>;

    /// Is the server answering? A `bool`, not a `Result`: every failure mode gives the same answer, and the caller only lights a dot.
    async fn health(&self) -> bool {
        true
    }

    /// The lifecycle half, if this provider has one. `None` means read-only: models live elsewhere.
    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        None
    }
}

/// Local model lifecycle.
#[async_trait]
pub trait ModelAdmin: Send + Sync {
    /// The model store, with load state and capabilities.
    async fn list(&self) -> Result<Vec<ModelInfo>, LlmError>;

    /// Models currently resident in VRAM.
    async fn running(&self) -> Result<Vec<RunningModel>, LlmError>;

    /// Authoritative metadata for one model.
    async fn show(&self, model: &str) -> Result<ModelDetails, LlmError>;

    /// Pull a model, emitting progress; a `BoxStream` because dropping it is the only sane way to abort a multi-gigabyte download.
    fn pull(&self, model: &str) -> BoxStream<'_, Result<PullProgress, LlmError>>;

    /// Release a model from VRAM.
    async fn unload(&self, model: &str) -> Result<(), LlmError>;

    /// Delete from disk.
    async fn delete(&self, model: &str) -> Result<(), LlmError>;
}

/// Seam: talking to a model.
pub enum Llm {}

impl ServiceKey for Llm {
    type Api = dyn LlmAdapter;
    const NAME: &'static str = "llm";
}

/// Seam: local model lifecycle.
pub enum Models {}

impl ServiceKey for Models {
    type Api = dyn ModelAdmin;
    const NAME: &'static str = "llm.models";
}
