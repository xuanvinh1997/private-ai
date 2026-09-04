//! pai-llm - message/stream vocabulary plus the model adapter seam.
//! [`message`]/[`stream`] are the provider-neutral contract, [`assembler`] folds chunks,
//! [`wire`] decodes sockets, and [`ollama`]/[`lmstudio`]/[`openai`] own the wire details.

pub mod assembler;
pub mod capabilities;
pub mod error;
pub mod lmstudio;
pub mod message;
pub mod model;
pub mod ollama;
pub mod openai;
pub mod registry;
pub mod seam;
pub mod stream;
pub mod wire;

pub use assembler::BlockAssembler;
pub use capabilities::{Capabilities, CapabilitySource};
pub use error::{LlmError, LlmErrorCode};
pub use message::{ChatRequest, ContentBlock, Message, ToolCall, ToolCallId, ToolSchema};
pub use model::{ModelDetails, ModelInfo, ModelState, PullProgress, RunningModel};
pub use lmstudio::{LmStudioAdapter, LmStudioAdmin};
pub use ollama::{OllamaAdapter, OllamaAdmin};
pub use openai::{OpenAiAdapter, openai_base_url};
pub use registry::{
    AdapterRegistry, ProviderConfig, ProviderKind, ProviderSignature, active_config,
};
pub use seam::{Llm, LlmAdapter, ModelAdmin, Models};
pub use stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};
