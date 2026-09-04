//! Built-in catalogue of known-good `base_url` + default-model pairs, because a mistyped URL only ever
//! surfaces as an unexplained network error. Three adapters, not ten: everything but Ollama and LM Studio
//! speaks the OpenAI protocol, and those two take a bare host root since they use two paths on it.

use pai_llm::{ProviderConfig, ProviderKind};

/// One catalogue entry.
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    pub needs_key: bool,
    /// Runs on this machine; duplicated from [`ProviderConfig::on_device`] so the UI can filter before any config exists.
    pub on_device: bool,
    /// A form prefill, not a claim the model still exists: the authoritative list comes from [`crate::probe`],
    /// and a hard-coded name ages with every provider release, so the UI must show it as editable.
    pub default_model: Option<&'static str>,
    /// Where to get a key, or where to download the server.
    pub homepage: &'static str,
    /// The one sentence a user needs before choosing.
    pub hint: &'static str,
}

impl Preset {
    /// Build a config from this entry; the store assigns the real `id`, so the preset id stands in for a pre-save probe.
    pub fn config(&self) -> ProviderConfig {
        ProviderConfig::new(self.id, self.name, self.kind, self.base_url)
    }
}

/// The entry with this exact `base_url`, used to guess a default model for a saved provider that has none.
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
        // Host root without `/v1`: `LmStudioAdapter` speaks both `/v1` and `/api/v0`; an older config keeping `/v1` still works.
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
