//! Which provider holds the vision role, and under what model name. The vision model reads page images for
//! OCR, so it is a separate decision from chat and embedding: not every chat model can see, and a scanned
//! page sent to a remote model is the whole page, not a query.

use pai_llm::ProviderKind;

use crate::store::StoredProvider;

/// Suggested vision model for an Ollama host; only a form prefill, since a given machine may not have it pulled.
pub const DEFAULT_VISION_MODEL_OLLAMA: &str = "qwen2.5vl:7b";

/// Suggested vision model for any OpenAI-protocol host.
pub const DEFAULT_VISION_MODEL_OPENAI: &str = "gpt-4o-mini";

/// Suggested vision model for LM Studio: their catalogue's name, not OpenAI's.
pub const DEFAULT_VISION_MODEL_LMSTUDIO: &str = "qwen2.5-vl-7b-instruct";

/// Per-kind suggestion, a prefill and nothing more.
pub fn default_vision_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => DEFAULT_VISION_MODEL_OLLAMA,
        ProviderKind::LmStudio => DEFAULT_VISION_MODEL_LMSTUDIO,
        ProviderKind::OpenAiCompatible => DEFAULT_VISION_MODEL_OPENAI,
    }
}

/// Why OCR cannot read images right now, or `None` when it can. Absence is a normal state, not an error:
/// documents with a text layer index fine, and images are simply left out until this is set.
pub fn vision_reason(provider: Option<&StoredProvider>) -> Option<String> {
    let Some(provider) = provider else {
        return Some(
            "Chưa chọn nhà cung cấp nào để đọc ảnh, nên ảnh và trang PDF đã quét sẽ bị bỏ \
             qua. Tài liệu có sẵn lớp chữ vẫn nạp bình thường."
                .to_string(),
        );
    };
    if !provider.config.enabled {
        return Some(format!(
            "Nhà cung cấp `{}` đang giữ vai đọc ảnh nhưng đã bị tắt. Bật lại nó, hoặc trao \
             vai đọc ảnh cho cái khác.",
            provider.config.name
        ));
    }
    if provider
        .vision_model
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Some(format!(
            "Nhà cung cấp `{}` chưa chọn mô hình đọc ảnh. Mô hình này phải nhìn được ảnh: \
             thử `{}`.",
            provider.config.name,
            default_vision_model(provider.config.kind)
        ));
    }
    None
}
