//! Bộ nhúng của provider đang giữ vai vai nhúng.
//!
//! Đây là nửa còn lại của việc tách hai vai. `pai-rag` biết nhúng nhưng cố ý không biết
//! provider nào; kho thì biết provider nhưng không biết `Embedder`. Một hàm ở giữa, và
//! **chỉ** ở giữa: không cache, không trạng thái, để chỗ gọi tự quyết định giữ kết quả bao
//! lâu.
//!
//! Luật quan trọng nhất ở đây là luật **không** làm: khi provider giữ vai nhúng chưa chọn
//! mô hình nhúng, hàm trả `None` thay vì mượn tạm `model` của vai hội thoại. `qwen3:8b`
//! không có endpoint embed; mượn nó là đổi một câu "chưa chọn mô hình nhúng" đọc được
//! thành một lỗi 400 ở mọi lần nạp tài liệu.

use std::sync::Arc;

use pai_llm::ProviderKind;
use pai_rag::{Embedder, OllamaEmbedder, OpenAiEmbedder};

use crate::store::StoredProvider;

/// Mô hình nhúng gợi ý cho một máy chủ Ollama.
///
/// Hằng số công khai vì giao diện điền sẵn nó vào ô nhập còn tầng dưới dùng nó để nói ra
/// gợi ý trong [`embedding_reason`]: hai chỗ hiện cùng một cái tên thì phải đọc cùng một
/// giá trị, nếu không thì người dùng thấy một tên và ứng dụng chờ một tên khác.
pub const DEFAULT_EMBEDDING_MODEL_OLLAMA: &str = "nomic-embed-text";

/// Mô hình nhúng gợi ý cho mọi máy chủ nói giao thức OpenAI.
pub const DEFAULT_EMBEDDING_MODEL_OPENAI: &str = "text-embedding-3-small";

/// Mô hình nhúng gợi ý cho LM Studio: tên trong kho của họ, không phải tên của OpenAI.
pub const DEFAULT_EMBEDDING_MODEL_LMSTUDIO: &str = "text-embedding-nomic-embed-text-v1.5";

/// Gợi ý theo loại provider. Chỉ là **gợi ý cho ô nhập**, cùng tinh thần với
/// [`crate::presets::Preset::default_model`]: một máy chủ tự vận hành có thể chẳng có mô
/// hình nào mang tên này.
pub fn default_embedding_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => DEFAULT_EMBEDDING_MODEL_OLLAMA,
        // LM Studio nhúng qua `/v1/embeddings` như mọi máy chủ OpenAI-compatible, nhưng
        // gợi ý thì khác: kho của nó không có `text-embedding-3-small` — đó là mô hình
        // của OpenAI — mà có bản GGUF của `nomic-embed-text`. Gợi ý sai còn tệ hơn không
        // gợi ý: người dùng dán nó vào rồi ngồi đọc một lỗi 404 không nói được vì sao.
        ProviderKind::LmStudio => DEFAULT_EMBEDDING_MODEL_LMSTUDIO,
        ProviderKind::OpenAiCompatible => DEFAULT_EMBEDDING_MODEL_OPENAI,
    }
}

/// Gốc máy chủ cho một endpoint nhúng.
///
/// `pai-rag` tự nối `/api/embed` và `/v1/embeddings`, nên cái nó cần là **gốc**. Kho thì
/// lưu URL theo dạng mà tầng hội thoại mong đợi, và phần lớn mục trong danh mục có đuôi
/// `/v1`. Cắt đuôi ấy ở đây là chỗ duy nhất biết cả hai quy ước — để nguyên thì mọi request
/// nhúng bay tới `/v1/v1/embeddings` và trả về 404 mà không ai đoán ra vì sao.
fn embedding_root(base_url: &str) -> String {
    let value = base_url.trim().trim_end_matches('/');
    let tail = value.rsplit('/').next().unwrap_or_default();
    let versioned =
        tail.starts_with('v') && tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit());
    if versioned {
        value[..value.len() - tail.len()]
            .trim_end_matches('/')
            .to_string()
    } else {
        value.to_string()
    }
}

/// Bộ nhúng cho provider đang giữ vai nhúng. `None` khi chưa ai giữ vai đó.
///
/// Cũng `None` khi provider ấy đang tắt hoặc chưa chọn mô hình nhúng — ba lý do khác nhau
/// cho cùng một câu trả lời, và [`embedding_reason`] là chỗ nói ra chúng khác nhau chỗ nào.
pub fn embedder_for(provider: &StoredProvider) -> Option<Arc<dyn Embedder>> {
    if !provider.config.enabled {
        return None;
    }
    let model = provider
        .embedding_model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())?;
    let root = embedding_root(&provider.config.base_url);
    Some(match provider.config.kind {
        ProviderKind::Ollama => Arc::new(OllamaEmbedder::new(root, model)),
        // LM Studio dùng chung bộ nhúng với nhánh OpenAI: `/v1/embeddings` của nó là
        // đúng endpoint ấy, đúng hình dạng thân ấy. Khác biệt của LM Studio nằm ở kho mô
        // hình, không nằm ở phép nhúng — nên ở đây không có gì phải viết thêm.
        ProviderKind::LmStudio | ProviderKind::OpenAiCompatible => Arc::new(OpenAiEmbedder::new(
            root,
            model,
            provider.config.api_key.clone(),
        )),
    })
}

/// Vì sao chưa nhúng được, khi chưa nhúng được. `None` nghĩa là đang sẵn sàng.
///
/// Có mặt vì [`embedder_for`] cố ý im lặng: nó trả `None` cho ba tình huống, và một giao
/// diện chỉ biết "không có bộ nhúng" thì không nói được cho người dùng phải bấm vào đâu.
/// Nhận `Option` chứ không nhận `&StoredProvider` để trả lời được cả trường hợp thường gặp
/// nhất — chưa ai giữ vai nhúng cả.
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
