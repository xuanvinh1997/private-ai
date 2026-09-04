//! Model-provider configuration and runtime switching: the half `pai-llm` deliberately omits, namely
//! where a `ProviderConfig` comes from -- on-disk store, presets, probing, and one path that keeps
//! disk, adapter cache and `Driver` in step. A swap takes effect from the next turn, not the next step.

pub mod embed;
pub mod error;
pub mod presets;
pub mod probe;
pub mod runtime;
pub mod seam;
pub mod store;

pub use embed::{
    DEFAULT_EMBEDDING_MODEL_OLLAMA, DEFAULT_EMBEDDING_MODEL_OPENAI, default_embedding_model,
    embedding_reason,
};
pub use error::{ProviderError, Result};
pub use presets::{PRESETS, Preset};
pub use probe::{EmbeddingProbeResult, ProbeModel, ProbeResult, probe, probe_embedding};
pub use runtime::{ModelListing, ProviderRuntime};
pub use seam::Providers;
pub use store::{DB_FILE, ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};
