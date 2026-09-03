//! Danh mục dựng sẵn.
//!
//! Lý do danh mục này tồn tại: `base_url` là chỗ người dùng gõ sai nhiều nhất, và cái giá
//! của một chữ sai là một thông báo lỗi mạng không nói được nguyên nhân. Mỗi mục ở đây là
//! một cặp URL + mô hình mặc định đã biết chắc đúng.
//!
//! **Chỉ có ba adapter, không phải mười.** Mọi mục dưới đây trừ Ollama và LM Studio đều
//! nói giao thức OpenAI, kể cả Anthropic: họ chạy một tầng tương thích ngay trên
//! `api.anthropic.com/v1`, nên `OpenAiAdapter` nói chuyện được với Claude mà không cần
//! một bản cài đặt thứ ba — và không cần ta nuôi thêm một wire format nữa qua từng bản
//! phát hành.
//!
//! LM Studio là adapter thứ ba, và nó **không** phải một wire format nữa: phần hội thoại
//! của nó đúng là dây của OpenAI, dùng lại nguyên vẹn. Nó tách ra vì nửa còn lại — kho
//! mô hình ở `/api/v0` — nói được điều mà `/v1/models` không nói được: mô hình nào đang
//! nạp, mô hình nào gọi được tool, cửa sổ ngữ cảnh bao nhiêu.
//!
//! `base_url` viết đúng dạng mà [`pai_llm::openai_base_url`] mong đợi: đuôi `/v1` được
//! giữ nguyên, thiếu thì nó tự thêm. Ollama và LM Studio là ngoại lệ — hai adapter ấy
//! nhận **gốc máy chủ** rồi tự nối đuôi, vì mỗi cái nói hai đường dẫn khác nhau trên
//! cùng một host.

use pai_llm::{ProviderConfig, ProviderKind};

/// Một mục trong danh mục.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    pub needs_key: bool,
    /// Chạy ngay trên máy này. Trùng với [`ProviderConfig::on_device`] nhưng có mặt ở đây
    /// để giao diện lọc được danh mục **trước khi** người dùng tạo ra cấu hình nào.
    pub on_device: bool,
    /// Chỉ là **gợi ý cho ô nhập**, không phải một khẳng định rằng mô hình này còn tồn
    /// tại. Danh sách có thẩm quyền đến từ [`crate::probe`] khi người dùng bấm thử kết
    /// nối; tên ở đây già đi theo từng bản phát hành của nhà cung cấp, và một hằng số
    /// trong mã nguồn thì không tự cập nhật được. Giao diện phải hiện nó như một giá trị
    /// điền sẵn sửa được, không phải như một lựa chọn đã chốt.
    pub default_model: Option<&'static str>,
    /// Chỗ lấy khoá, hoặc chỗ tải máy chủ về.
    pub homepage: &'static str,
    /// Một câu người dùng cần biết **trước khi** chọn.
    pub hint: &'static str,
}

impl Preset {
    /// Dựng một cấu hình từ mục này. `id` của cấu hình do kho đặt, nên ở đây dùng tạm id
    /// của mục — đủ để thử một phát trước khi lưu.
    pub fn config(&self) -> ProviderConfig {
        ProviderConfig::new(self.id, self.name, self.kind, self.base_url)
    }
}

/// Mục mang đúng `base_url` này, nếu có. Dùng để đoán mô hình mặc định cho một provider
/// người dùng đã lưu mà chưa chọn mô hình.
pub fn matching(base_url: &str) -> Option<&'static Preset> {
    let needle = base_url.trim().trim_end_matches('/');
    PRESETS.iter().find(|preset| {
        preset
            .base_url
            .trim_end_matches('/')
            .eq_ignore_ascii_case(needle)
    })
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "ollama",
        name: "Ollama",
        kind: ProviderKind::Ollama,
        base_url: "http://localhost:11434",
        needs_key: false,
        on_device: true,
        default_model: Some("qwen3:8b"),
        homepage: "https://ollama.com/download",
        hint: "Chạy hoàn toàn trên máy bạn: không có gì rời khỏi đây. Cần cài Ollama và \
               kéo mô hình về trước.",
    },
    Preset {
        id: "lmstudio",
        name: "LM Studio",
        kind: ProviderKind::LmStudio,
        // Gốc máy chủ, không có `/v1`: `LmStudioAdapter` nói cả `/v1` lẫn `/api/v0` nên
        // nó phải nhận gốc. Một cấu hình cũ còn đuôi `/v1` vẫn chạy — adapter cắt đuôi.
        base_url: "http://localhost:1234",
        needs_key: false,
        on_device: true,
        default_model: None,
        homepage: "https://lmstudio.ai",
        hint: "Chạy trên máy bạn. Phải bật máy chủ cục bộ trong tab Developer của LM \
               Studio thì địa chỉ này mới có ai trả lời.",
    },
    Preset {
        id: "llamacpp",
        name: "llama.cpp server",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:8080/v1",
        needs_key: false,
        on_device: true,
        default_model: None,
        homepage: "https://github.com/ggml-org/llama.cpp",
        hint: "Chạy trên máy bạn. `llama-server` phục vụ đúng một mô hình — cái bạn truyền \
               cho nó lúc khởi động — nên tên mô hình ở đây gần như không quan trọng.",
    },
    Preset {
        id: "vllm",
        name: "vLLM",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:8000/v1",
        needs_key: false,
        on_device: true,
        default_model: None,
        homepage: "https://docs.vllm.ai",
        hint: "Máy chủ tự vận hành, thường trên một máy có GPU. Tên mô hình phải trùng \
               đúng cái đã nạp lúc khởi động vLLM.",
    },
    Preset {
        id: "openai",
        name: "OpenAI",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("gpt-5.5"),
        homepage: "https://platform.openai.com/api-keys",
        hint: "Dịch vụ trả tiền theo lượng dùng. Mọi thứ bạn gửi đi đều rời khỏi máy này.",
    },
    Preset {
        id: "anthropic",
        name: "Anthropic",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.anthropic.com/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("claude-sonnet-5"),
        homepage: "https://console.anthropic.com/settings/keys",
        hint: "Đi qua tầng tương thích OpenAI của chính Anthropic, nên nó nhận phần lớn \
               nhưng không phải mọi tính năng của API gốc. Dịch vụ trả tiền.",
    },
    Preset {
        id: "openrouter",
        name: "OpenRouter",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("openai/gpt-5.5"),
        homepage: "https://openrouter.ai/keys",
        hint: "Một khoá dùng được nhiều nhà cung cấp. Tên mô hình phải có tiền tố hãng, \
               ví dụ `anthropic/claude-sonnet-4.5`.",
    },
    Preset {
        id: "deepseek",
        name: "DeepSeek",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("deepseek-chat"),
        homepage: "https://platform.deepseek.com/api_keys",
        hint: "Dịch vụ trả tiền, giá thấp. Máy chủ đặt ngoài lãnh thổ bạn — cân nhắc với \
               mã nguồn nội bộ.",
    },
    Preset {
        id: "groq",
        name: "Groq",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.groq.com/openai/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("llama-3.3-70b-versatile"),
        homepage: "https://console.groq.com/keys",
        hint: "Rất nhanh, nhưng chỉ phục vụ mô hình mở và có hạn mức theo phút.",
    },
    Preset {
        id: "xai",
        name: "xAI",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.x.ai/v1",
        needs_key: true,
        on_device: false,
        default_model: Some("grok-4"),
        homepage: "https://console.x.ai",
        hint: "Dịch vụ trả tiền, dùng chung khoá với bảng điều khiển xAI.",
    },
];
