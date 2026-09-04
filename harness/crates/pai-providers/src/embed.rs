//! Which provider holds the embedding role, and under what model name. This module answers
//! who, which model, which host, and why it is not ready.
//! Never borrow the chat role's `model`: it has no embed endpoint, turning a clear message into a 400.

use pai_llm::ProviderKind;

use crate::store::StoredProvider;

/// Suggested embedding model for an Ollama host; public so the UI prefill and [`embedding_reason`] read
/// one value. `qwen3-embedding:4b` rather than the English-leaning `nomic-embed-text`, since the document
/// library is Vietnamese.
pub const DEFAULT_EMBEDDING_MODEL_OLLAMA: &str = "qwen3-embedding:4b";

/// Suggested embedding model for any OpenAI-protocol host.
pub const DEFAULT_EMBEDDING_MODEL_OPENAI: &str = "text-embedding-3-small";

/// Suggested embedding model for LM Studio: their catalogue's name, not OpenAI's.
pub const DEFAULT_EMBEDDING_MODEL_LMSTUDIO: &str = "text-embedding-nomic-embed-text-v1.5";

/// Per-kind suggestion, only a form prefill like [`crate::presets::Preset::default_model`]: a self-hosted server may have no such model.
pub fn default_embedding_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => DEFAULT_EMBEDDING_MODEL_OLLAMA,
        // LM Studio embeds via `/v1/embeddings` like any OpenAI-compatible host, but its catalogue has GGUF
        // `nomic-embed-text`, not OpenAI's `text-embedding-3-small`; a wrong suggestion just yields a 404.
        ProviderKind::LmStudio => DEFAULT_EMBEDDING_MODEL_LMSTUDIO,
        ProviderKind::OpenAiCompatible => DEFAULT_EMBEDDING_MODEL_OPENAI,
    }
}

/// Why embedding is not ready, or `None` when it is: three distinct situations share that state and the
/// UI must tell the user where to click. Takes an `Option` so it covers the common "nobody holds the role".
pub fn embedding_reason(provider: Option<&StoredProvider>) -> Option<String> {
    let Some(provider) = provider else {
        return Some(
            "Chưa chọn nhà cung cấp nào để nhúng tài liệu, nên thư viện chỉ tìm theo từ \
             khoá. Chọn một cái ở mục Nhà cung cấp — nhúng bằng một mô hình chạy trên máy \
             này thì tài liệu không đi đâu cả."
                .to_string(),
        );
    };
    if !provider.config.enabled {
        return Some(format!(
            "Nhà cung cấp `{}` đang giữ vai nhúng nhưng đã bị tắt. Bật lại nó, hoặc trao \
             vai nhúng cho cái khác.",
            provider.config.name
        ));
    }
    if provider
        .embedding_model
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Some(format!(
            "Nhà cung cấp `{}` chưa chọn mô hình nhúng. Mô hình nhúng khác mô hình trò \
             chuyện: thử `{}`.",
            provider.config.name,
            default_embedding_model(provider.config.kind)
        ));
    }
    None
}
