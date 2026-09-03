//! Provider nào đang giữ vai nhúng, và tên mô hình nhúng của nó.
//!
//! Module này **không** nhúng gì cả — việc ấy nằm ở `services/rag/`, một tiến trình
//! Python. Ở đây chỉ còn phần mà kho provider trả lời được: ai giữ vai, mô hình tên gì,
//! gốc máy chủ ở đâu, và vì sao chưa sẵn sàng. Ba thứ đó được ghi vào tệp cấu hình mà
//! service đọc.
//!
//! Luật quan trọng nhất vẫn là luật **không** làm: khi provider giữ vai nhúng chưa chọn
//! mô hình nhúng, đừng mượn tạm `model` của vai hội thoại. `qwen3:8b` không có endpoint
//! embed; mượn nó là đổi một câu "chưa chọn mô hình nhúng" đọc được thành một lỗi 400 ở
//! mọi lần nạp tài liệu.


use pai_llm::ProviderKind;

use crate::store::StoredProvider;

/// Mô hình nhúng gợi ý cho một máy chủ Ollama.
///
/// Hằng số công khai vì giao diện điền sẵn nó vào ô nhập còn tầng dưới dùng nó để nói ra
/// gợi ý trong [`embedding_reason`]: hai chỗ hiện cùng một cái tên thì phải đọc cùng một
/// giá trị, nếu không thì người dùng thấy một tên và ứng dụng chờ một tên khác.
/// `qwen3-embedding:4b` chứ không phải `nomic-embed-text`.
///
/// `nomic-embed-text` thiên về tiếng Anh, trong khi thư viện tài liệu ở đây là tiếng
/// Việt. Đây là một trong hai bản vá về chất lượng truy hồi đi cùng việc chuyển tầng RAG
/// sang `services/rag/` — cái còn lại là tiền tố bất đối xứng cho câu hỏi và cho đoạn
/// (xem `pai_rag_service.embed.PREFIXES`). Giữ khớp với `DEFAULT_EMBED_MODEL` bên đó.
pub const DEFAULT_EMBEDDING_MODEL_OLLAMA: &str = "qwen3-embedding:4b";

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

/// Vì sao chưa nhúng được, khi chưa nhúng được. `None` nghĩa là đang sẵn sàng.
///
/// Có mặt vì ba tình huống khác nhau cùng dẫn tới "chưa nhúng được", và một giao diện
/// chỉ biết chừng ấy thì không nói được cho người dùng phải bấm vào đâu. Chuỗi này đi
/// vào tệp cấu hình của `pai-rag-service` và lên thẳng dải trạng thái thư viện.
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
