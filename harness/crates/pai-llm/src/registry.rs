//! Provider đã cấu hình, và cache adapter dựng từ chúng.
//!
//! Port `llm/registry.py` + phần cache của `llm/router.py`. Ở đây **không có SQLite**:
//! crate này nhận `ProviderConfig` đã đọc sẵn. Tách như vậy vì lưu ở đâu là chuyện của
//! tầng lưu trữ, còn "dựng adapter nào cho cấu hình này" là chuyện của tầng mô hình, và
//! trộn hai thứ lại là lý do bản Python phải chuyền một `Database` xuống tận `ModelRouter`.
//!
//! Cái phải giữ nguyên từ bản gốc là **cache đánh khoá theo chữ ký, không theo id**
//! (`router.py:158-167`). Người dùng sửa URL của một provider mà id giữ nguyên: nếu cache
//! khoá theo id thì mọi request tiếp theo vẫn bay tới máy chủ cũ, và không có gì báo động.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::error::{LlmError, LlmErrorCode};
use crate::lmstudio::LmStudioAdapter;
use crate::ollama::OllamaAdapter;
use crate::openai::OpenAiAdapter;
use crate::seam::{LlmAdapter, ModelAdmin};

/// Loại máy chủ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Ollama,
    /// LM Studio. Nói giao thức OpenAI ở phần hội thoại, nhưng có **kho mô hình riêng**
    /// ở `/api/v0` — xem [`crate::lmstudio`]. Tách thành một loại riêng vì đó đúng là
    /// khác biệt mà người dùng cảm thấy: cùng một máy chủ, một bên biết mô hình nào đang
    /// nạp và làm được gì, một bên chỉ đọc được cái tên.
    LmStudio,
    /// Bất cứ thứ gì nói giao thức OpenAI: llama.cpp, vLLM, OpenAI thật.
    OpenAiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
            Self::OpenAiCompatible => "openai",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "ollama" => Some(Self::Ollama),
            "lmstudio" | "lm_studio" | "lm-studio" | "lm studio" => Some(Self::LmStudio),
            "openai" | "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

/// Mọi thứ mà một adapter đã dựng phụ thuộc vào.
///
/// `Debug` được viết tay để **không in khoá API**: cấu trúc này lọt vào log lỗi, và một
/// khoá trong log là một khoá đã rò rỉ.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProviderSignature {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    api_key: String,
}

impl fmt::Debug for ProviderSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderSignature")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "<trống>"
                } else {
                    "<đã đặt>"
                },
            )
            .finish()
    }
}

/// Một máy chủ đã cấu hình.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub enabled: bool,
}

impl ProviderConfig {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: ProviderKind,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            base_url: base_url.into(),
            api_key: String::new(),
            enabled: true,
        }
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn signature(&self) -> ProviderSignature {
        ProviderSignature {
            id: self.id.clone(),
            kind: self.kind,
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
        }
    }

    /// Provider chạy ngay trên máy này khi endpoint không rời interface loopback.
    /// Port `runs_on_device` (`registry.py:41-45`) — giao diện dùng nó để nói với người
    /// dùng rằng dữ liệu không đi đâu cả.
    pub fn on_device(&self) -> bool {
        const LOOPBACK: [&str; 5] = ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"];
        let without_scheme = self
            .base_url
            .split("://")
            .nth(1)
            .unwrap_or(self.base_url.as_str());
        let authority = without_scheme.split('/').next().unwrap_or_default();
        // Cắt cổng, nhưng chừa `[::1]` vốn có dấu hai chấm bên trong ngoặc vuông.
        let host = if let Some(rest) = authority.strip_prefix('[') {
            rest.split(']')
                .next()
                .map(|h| format!("[{h}]"))
                .unwrap_or_default()
        } else {
            authority.split(':').next().unwrap_or_default().to_string()
        };
        LOOPBACK.contains(&host.to_lowercase().as_str())
    }
}

/// Provider đang được chọn.
///
/// Port `ProviderRegistry.active_config` (`registry.py:140-153`), giữ nguyên cả ba tầng
/// dự phòng: cái được ghim, rồi cái đầu tiên còn bật, rồi cái đầu tiên bất kể. Tầng chót
/// là cố ý — một provider bị tắt vẫn tốt hơn một thông báo "chưa cấu hình gì cả" khi
/// người dùng rõ ràng đã cấu hình.
pub fn active_config<'a>(
    configs: &'a [ProviderConfig],
    selected_id: &str,
) -> Option<&'a ProviderConfig> {
    if configs.is_empty() {
        return None;
    }
    configs
        .iter()
        .find(|config| config.id == selected_id.trim() && config.enabled)
        .or_else(|| configs.iter().find(|config| config.enabled))
        .or_else(|| configs.first())
}

/// Dựng adapter, và giữ lại cái đã dựng.
///
/// Dựng một adapter không đắt bằng `ChatOllama` của Python, nhưng cache vẫn cần: nó là
/// nơi giữ connection pool của `reqwest`, và một adapter mới mỗi lượt nghĩa là bắt tay
/// TLS lại từ đầu mỗi lượt.
pub struct AdapterRegistry {
    http: reqwest::Client,
    cache: Mutex<HashMap<ProviderSignature, Arc<dyn LlmAdapter>>>,
}

impl AdapterRegistry {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Adapter cho một cấu hình, dựng nếu chưa có.
    pub fn adapter(&self, config: &ProviderConfig) -> Result<Arc<dyn LlmAdapter>, LlmError> {
        let signature = config.signature();
        {
            let cache = self.lock();
            if let Some(adapter) = cache.get(&signature) {
                return Ok(adapter.clone());
            }
        }
        let adapter: Arc<dyn LlmAdapter> = match config.kind {
            ProviderKind::Ollama => Arc::new(OllamaAdapter::new(
                config.id.clone(),
                &config.base_url,
                self.http.clone(),
            )),
            ProviderKind::LmStudio => Arc::new(LmStudioAdapter::new(
                config.id.clone(),
                &config.base_url,
                config.api_key.clone(),
                self.http.clone(),
            )),
            ProviderKind::OpenAiCompatible => Arc::new(OpenAiAdapter::new(
                config.id.clone(),
                &config.base_url,
                config.api_key.clone(),
                self.http.clone(),
            )?),
        };
        let mut cache = self.lock();
        // Vứt mọi thứ dựng từ một hình dạng cũ của **chính provider này**. Provider khác
        // không bị đụng: chúng có id khác, và cái đang chạy trên chúng vẫn hợp lệ.
        cache.retain(|key, _| key.id != signature.id);
        cache.insert(signature, adapter.clone());
        Ok(adapter)
    }

    /// Nửa vòng đời của một provider.
    ///
    /// Port `ModelAdmin.provider` (`admin.py:62-68`): provider từ xa trả lời bằng một câu
    /// tiếng Việt nói rõ *vì sao* không áp dụng, chứ không phải một `None` câm lặng.
    pub fn admin(&self, config: &ProviderConfig) -> Result<Arc<dyn ModelAdmin>, LlmError> {
        self.adapter(config)?.admin().ok_or_else(|| {
            LlmError::read_only(format!(
                "{} lưu mô hình từ xa; thao tác này không áp dụng",
                config.name
            ))
        })
    }

    /// Số adapter đang giữ. Dành cho bài test và cho `--dump-config`.
    pub fn cached(&self) -> usize {
        self.lock().len()
    }

    /// Khoá bị nhiễm độc nghĩa là một luồng khác đã hoảng khi đang giữ nó. Dữ liệu bên
    /// trong vẫn nhất quán — chỉ là một `HashMap` — nên lấy lại mà dùng, thay vì lan
    /// truyền một cú hoảng nữa. Không `unwrap()` trên đường chạy thật.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ProviderSignature, Arc<dyn LlmAdapter>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Không còn provider nào. Tách thành hàm để thông điệp chỉ có một bản.
pub fn no_provider() -> LlmError {
    LlmError::new(
        LlmErrorCode::NoProviderConfigured,
        "Chưa cấu hình nhà cung cấp AI nào",
    )
}
