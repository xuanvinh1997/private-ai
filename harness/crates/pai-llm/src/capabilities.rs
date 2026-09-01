//! Mô hình này làm được gì.
//!
//! Hai nguồn sự thật, **theo đúng thứ tự này** (port từ `llm/capabilities.py`):
//!
//! 1. `/api/show` của Ollama báo năng lực có thẩm quyền — nó đọc từ chính tệp GGUF.
//! 2. Đoán theo tên, chỉ khi hỏi không được: một máy chủ OpenAI-compatible chỉ liệt kê
//!    id và `owned_by`, còn một bản Ollama cũ thì không có trường `capabilities`.
//!
//! Thứ tự này quan trọng. Đoán theo tên là suy luận trên chuỗi ký tự do người khác đặt:
//! nó đúng cho `llava`, sai cho một bản fine-tune tên là `cong-ty-cua-toi:latest`. Chỉ
//! được dùng khi không còn cách nào khác.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Tập năng lực mà Ollama báo. Cái nào không nằm trong đây thì bỏ, vì nó là từ vựng của
/// một phiên bản Ollama mới hơn cái ta biết.
pub const OLLAMA_CAPABILITIES: [&str; 5] = ["chat", "embedding", "vision", "tools", "thinking"];

/// Ollama gọi sinh văn bản thuần là "completion"; phần còn lại của ứng dụng gọi là chat.
const OLLAMA_ALIASES: [(&str, &str); 1] = [("completion", "chat")];

/// Chuỗi con báo hiệu một mô hình nhìn được ảnh. Chép nguyên từ `capabilities.py:27-38` —
/// danh sách này là kinh nghiệm tích luỹ, không phải suy luận, nên đừng "dọn dẹp" nó.
const VISION_TOKENS: [&str; 11] = [
    "-vl",
    ":vl",
    "clip",
    "gemma3",
    "gpt-4o",
    "gpt-5",
    "llava",
    "minicpm-v",
    "moondream",
    "o4-mini",
    "vision",
];

/// Năng lực đến từ đâu. Giao diện cần biết: "mô hình này không gọi được tool" là một câu
/// khác hẳn khi nó là sự thật đọc từ tệp và khi nó chỉ là phỏng đoán từ cái tên.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    /// Máy chủ tự khai.
    Reported,
    /// Đoán từ tên mô hình.
    Inferred,
}

/// Năng lực của một mô hình.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub chat: bool,
    pub embedding: bool,
    /// Nhìn được ảnh.
    pub vision: bool,
    /// Gọi được tool. Vòng lặp agent đọc trường này để biết có nên trao schema tool không.
    pub tools: bool,
    /// Có kênh suy luận riêng.
    pub thinking: bool,
    /// Cửa sổ ngữ cảnh, tính bằng token. `None` khi không hỏi được — và `None` **không**
    /// có nghĩa là vô hạn; nó có nghĩa là chưa biết, nên người gọi phải tự chọn mặc định.
    pub context_window: Option<u64>,
    pub source: CapabilitySource,
}

impl Capabilities {
    /// Bộ khung rỗng.
    fn empty(source: CapabilitySource) -> Self {
        Self {
            chat: false,
            embedding: false,
            vision: false,
            tools: false,
            thinking: false,
            context_window: None,
            source,
        }
    }

    /// Đoán từ một chuỗi mô tả (tên mô hình, cộng bất cứ metadata nào máy chủ tự nguyện
    /// đưa thêm). Port `infer_capabilities` — giữ nguyên cả thứ tự nhánh: "embed" thắng
    /// trước, vì `nomic-embed-vision` là mô hình nhúng chứ không phải mô hình thị giác.
    pub fn infer(descriptor: &str) -> Self {
        let value = descriptor.to_lowercase();
        let mut caps = Self::empty(CapabilitySource::Inferred);
        if value.contains("embed") {
            caps.embedding = true;
            return caps;
        }
        caps.chat = true;
        caps.vision = VISION_TOKENS.iter().any(|token| value.contains(token));
        caps
    }

    /// Đọc mảng `capabilities` của `/api/show`.
    ///
    /// Trả `None` khi không lọc ra được năng lực nào — đó là tín hiệu để người gọi rơi
    /// xuống nhánh đoán, đúng như `admin.py:167-169` làm với `if capabilities:`.
    pub fn from_reported(reported: &[String], context_window: Option<u64>) -> Option<Self> {
        let mut caps = Self::empty(CapabilitySource::Reported);
        let mut any = false;
        for raw in reported {
            let lowered = raw.to_lowercase();
            let name = OLLAMA_ALIASES
                .iter()
                .find(|(from, _)| *from == lowered)
                .map(|(_, to)| *to)
                .unwrap_or(lowered.as_str());
            if !OLLAMA_CAPABILITIES.contains(&name) {
                continue;
            }
            any = true;
            match name {
                "chat" => caps.chat = true,
                "embedding" => caps.embedding = true,
                "vision" => caps.vision = true,
                "tools" => caps.tools = true,
                "thinking" => caps.thinking = true,
                _ => {}
            }
        }
        if !any {
            return None;
        }
        caps.context_window = context_window;
        Some(caps)
    }

    /// Chỉ nhúng, không chat. Bản Python phân loại `model_type` bằng đúng phép so sánh
    /// `capabilities == ["embedding"]`.
    pub fn is_embedding_only(&self) -> bool {
        self.embedding && !self.chat
    }

    /// Danh sách tên, để hiện ra giao diện và ghi vào cơ sở dữ liệu.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.chat {
            names.push("chat");
        }
        if self.embedding {
            names.push("embedding");
        }
        if self.vision {
            names.push("vision");
        }
        if self.tools {
            names.push("tools");
        }
        if self.thinking {
            names.push("thinking");
        }
        names
    }
}

/// Lọc mảng `capabilities` thô thành danh sách tên đã chuẩn hoá, giữ thứ tự và loại trùng.
/// Port thẳng `normalize_ollama_capabilities`; hữu ích khi chỉ cần danh sách chứ không
/// cần cả cấu trúc.
pub fn normalize_ollama_capabilities(value: &Value) -> Vec<String> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let Some(raw) = item.as_str() else { continue };
        let lowered = raw.to_lowercase();
        let name = OLLAMA_ALIASES
            .iter()
            .find(|(from, _)| *from == lowered)
            .map(|(_, to)| (*to).to_string())
            .unwrap_or(lowered);
        if OLLAMA_CAPABILITIES.contains(&name.as_str()) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Tìm cửa sổ ngữ cảnh trong `model_info` của `/api/show`.
///
/// Khoá có tiền tố là tên kiến trúc — `llama.context_length`, `qwen3.context_length`,
/// `gemma3.context_length` — nên không tra thẳng được. Khớp theo đuôi là cách duy nhất
/// không phải giữ một bảng kiến trúc rồi phải cập nhật mỗi lần có mô hình mới.
pub fn context_length_from_model_info(info: &Map<String, Value>) -> Option<u64> {
    info.iter()
        .find(|(key, _)| key.ends_with(".context_length") || *key == "context_length")
        .and_then(|(_, value)| value.as_u64())
}
