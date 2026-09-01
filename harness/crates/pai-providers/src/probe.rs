//! Thử một cấu hình **trước khi** lưu nó.
//!
//! Người dùng đang đứng chờ một cái nút, nên hai luật:
//!
//! 1. **Thời hạn ngắn.** Một máy chủ cục bộ chưa bật thường từ chối kết nối ngay, nhưng
//!    một địa chỉ gõ sai thành một host không tồn tại thì treo tới khi DNS bỏ cuộc. Bốn
//!    giây là ngưỡng: đủ cho một chuyến đi vòng qua Đại Tây Dương, không đủ để người dùng
//!    tưởng ứng dụng chết.
//! 2. **Ba tình huống, ba câu khác nhau**, vì chúng đòi ba hành động khác nhau: bật máy
//!    chủ lên, sửa khoá, hoặc tải một mô hình về. Một câu "không kết nối được" chung chung
//!    cho cả ba là một câu vô dụng.

use std::time::Duration;

use pai_llm::{Capabilities, LlmErrorCode, ProviderConfig, ProviderKind, openai_base_url};
use serde_json::Value;

/// Người dùng chờ được bao lâu trước khi nghĩ là ứng dụng treo.
const TIMEOUT: Duration = Duration::from_secs(4);

/// Một mô hình mà máy chủ khai là nó có.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeModel {
    pub id: String,
    /// **Phỏng đoán từ tên**, không phải sự thật: một lần thử không trả tiền cho một lượt
    /// `/api/show` trên từng mô hình. Giá trị có thẩm quyền đến sau, từ
    /// [`pai_llm::LlmAdapter::capabilities`], khi provider đã được chọn thật.
    pub tools: bool,
    pub context_window: Option<u64>,
}

/// Kết quả một lần thử.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeResult {
    pub ok: bool,
    /// Một câu tiếng Việt nói **phải làm gì tiếp theo**, không phải nói lỗi gì.
    pub message: String,
    pub models: Vec<ProbeModel>,
}

impl ProbeResult {
    fn fail(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            models: Vec::new(),
        }
    }
}

/// Gọi endpoint liệt kê mô hình của một cấu hình.
///
/// Không đi qua [`pai_llm::AdapterRegistry`]: cấu hình đang thử **chưa được lưu**, và
/// nhét nó vào cache adapter nghĩa là một URL gõ sai vẫn nằm lại đó sau khi người dùng
/// đã bỏ hộp thoại đi.
pub async fn probe(config: &ProviderConfig, http: &reqwest::Client) -> ProbeResult {
    let (url, authorized) = match config.kind {
        ProviderKind::Ollama => (
            format!("{}/api/tags", config.base_url.trim_end_matches('/')),
            false,
        ),
        ProviderKind::OpenAiCompatible => match openai_base_url(&config.base_url) {
            Ok(root) => (format!("{root}/models"), !config.api_key.is_empty()),
            Err(err) => return ProbeResult::fail(err.message),
        },
    };

    let mut request = http.get(&url).timeout(TIMEOUT);
    if authorized {
        request = request.bearer_auth(&config.api_key);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            // Nhóm **không nối được**: chưa có ai nghe ở địa chỉ này. Khoá đúng hay sai
            // chưa hề được hỏi tới, nên đừng nhắc tới khoá ở đây — đó là cách người dùng
            // đi sửa nhầm chỗ trong nửa tiếng.
            let detail = if err.is_timeout() {
                "máy chủ không trả lời kịp"
            } else {
                "không mở được kết nối"
            };
            return ProbeResult::fail(format!(
                "Không nối được tới {url} ({detail}). Kiểm tra máy chủ đã chạy chưa và địa \
                 chỉ có đúng không."
            ));
        }
    };

    let status = response.status().as_u16();
    if !status_ok(status) {
        let body = response.text().await.unwrap_or_default();
        let err = pai_llm::LlmError::from_status(status, &body);
        return ProbeResult::fail(match err.code {
            // Nhóm **sai khoá**: máy chủ có đó, nói chuyện được, và từ chối ta.
            LlmErrorCode::Auth if config.api_key.is_empty() => {
                format!("Máy chủ ở {url} đòi khoá API mà cấu hình này chưa có khoá.")
            }
            LlmErrorCode::Auth => {
                format!("Máy chủ ở {url} từ chối khoá API này (HTTP {status}). Kiểm tra lại khoá.")
            }
            _ => format!("Máy chủ ở {url} trả về lỗi: {}", err.message),
        });
    }

    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => return ProbeResult::fail(format!("Không đọc được phản hồi từ {url}: {err}")),
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return ProbeResult::fail(format!(
                "{url} trả lời nhưng không phải JSON của một API mô hình. Địa chỉ này có \
                 thể trỏ vào một dịch vụ khác."
            ));
        }
    };

    let models = match config.kind {
        ProviderKind::Ollama => parse_names(&payload, "models", "name"),
        ProviderKind::OpenAiCompatible => parse_names(&payload, "data", "id"),
    };

    if models.is_empty() {
        // Nhóm **rỗng**: nối được, xác thực xong, nhưng không có gì để chạy. Việc phải làm
        // là kéo một mô hình về, không phải sửa cấu hình — nên câu chữ phải nói ra điều đó.
        return ProbeResult::fail(match config.kind {
            ProviderKind::Ollama => format!(
                "Nối được tới {url}, nhưng máy chủ chưa có mô hình nào. Kéo một cái về bằng \
                 `ollama pull` trước đã."
            ),
            ProviderKind::OpenAiCompatible => format!(
                "Nối được tới {url}, nhưng máy chủ không khai mô hình nào. Với một máy chủ \
                 tự vận hành thì đó thường là do chưa nạp mô hình lúc khởi động."
            ),
        });
    }

    ProbeResult {
        ok: true,
        message: format!("Nối được, máy chủ khai {} mô hình.", models.len()),
        models,
    }
}

fn status_ok(status: u16) -> bool {
    (200..300).contains(&status)
}

fn parse_names(payload: &Value, array: &str, field: &str) -> Vec<ProbeModel> {
    payload
        .get(array)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get(field).and_then(Value::as_str)?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    let caps = Capabilities::infer(id);
                    Some(ProbeModel {
                        id: id.to_string(),
                        tools: caps.tools,
                        context_window: caps.context_window,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
