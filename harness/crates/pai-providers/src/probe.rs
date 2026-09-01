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
//!
//! [`probe_embedding`] là phép thử của vai nhúng, và nó làm một việc khác hẳn: nó **nhúng
//! thật một câu** thay vì liệt kê mô hình. Lý do nằm ở chính doc của nó.

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

/// Bao lâu cho một lần thử nhúng.
///
/// Dài hơn [`TIMEOUT`] vì lần nhúng đầu tiên của một máy chủ cục bộ còn phải nạp mô hình
/// vào VRAM, và một lần thử hỏng vì hết giờ trong khi máy chủ vẫn đang nạp là một câu trả
/// lời sai. Vẫn ngắn, vì vẫn có người đang đứng chờ một cái nút.
const EMBED_TIMEOUT: Duration = Duration::from_secs(8);

/// Câu đem đi nhúng thử.
///
/// Tiếng Việt có dấu, cố ý: một máy chủ nhúng cấu hình sai bộ token hoá vẫn nuốt trôi
/// `hello` nhưng chết ở đây, và biết điều đó ngay lúc bấm nút thì tốt hơn nhiều so với
/// biết sau khi đã nạp cả thư viện tài liệu.
const EMBED_SAMPLE: &str = "Một câu tiếng Việt ngắn để thử.";

/// Kết quả một lần thử **nhúng thật**.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingProbeResult {
    pub ok: bool,
    /// Một câu tiếng Việt nói **phải làm gì tiếp theo**.
    pub message: String,
    /// Số chiều đo được từ vector thật trả về.
    pub dimensions: Option<usize>,
}

impl EmbeddingProbeResult {
    fn fail(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            dimensions: None,
        }
    }
}

/// Gửi một câu đi nhúng và đo số chiều của vector trả về.
///
/// **Không liệt kê mô hình.** Liệt kê không trả lời được câu hỏi thật: `/api/tags` của
/// Ollama trả về *mọi* mô hình và không có gì trong đó nói cái nào nhúng được, nên một danh
/// sách đẹp vẫn để người dùng chọn nhầm `llama3` rồi ngồi nhìn mọi lần nạp tài liệu thất
/// bại. Cách duy nhất biết chắc là gửi một câu đi và xem có vector về không.
///
/// Bốn tình huống, bốn câu khác nhau, vì chúng đòi bốn hành động khác nhau: bật máy chủ,
/// sửa khoá, kéo mô hình về, hay đổi sang một mô hình nhúng thật.
pub async fn probe_embedding(config: &ProviderConfig, model: &str) -> EmbeddingProbeResult {
    let model = model.trim();
    if model.is_empty() {
        return EmbeddingProbeResult::fail(
            "Chưa có tên mô hình nhúng để thử. Mô hình nhúng khác mô hình trò chuyện — nó \
             phải được chọn riêng.",
        );
    }
    let (url, authorized) = match config.kind {
        ProviderKind::Ollama => (format!("{}/api/embed", embed_root(&config.base_url)), false),
        ProviderKind::OpenAiCompatible => match openai_base_url(&config.base_url) {
            Ok(root) => (format!("{root}/embeddings"), !config.api_key.is_empty()),
            Err(err) => return EmbeddingProbeResult::fail(err.message),
        },
    };

    // Client dựng tại chỗ: cấu hình đang thử có thể chưa từng được lưu, và một lần bấm nút
    // không đáng để kéo theo một tham số client qua cả chuỗi lời gọi ở tầng trên.
    let http = reqwest::Client::builder()
        .timeout(EMBED_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut request = http
        .post(&url)
        .timeout(EMBED_TIMEOUT)
        .json(&serde_json::json!({ "model": model, "input": [EMBED_SAMPLE] }));
    if authorized {
        request = request.bearer_auth(&config.api_key);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            // Nhóm **không nối được**, cùng luật với [`probe`]: khoá chưa hề được hỏi tới,
            // nên đừng nhắc tới khoá ở đây.
            let detail = if err.is_timeout() {
                "máy chủ không trả lời kịp"
            } else {
                "không mở được kết nối"
            };
            return EmbeddingProbeResult::fail(format!(
                "Không nối được tới {url} ({detail}). Kiểm tra máy chủ đã chạy chưa và địa \
                 chỉ có đúng không."
            ));
        }
    };

    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if !status_ok(status) {
        return EmbeddingProbeResult::fail(explain(config, model, &url, status, &body));
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return EmbeddingProbeResult::fail(format!(
                "{url} trả lời nhưng không phải JSON. Địa chỉ này có thể trỏ vào một dịch \
                 vụ khác."
            ));
        }
    };

    match first_vector(config.kind, &payload) {
        // Nhóm **tồn tại nhưng không nhúng được** ở dạng khó chịu nhất: máy chủ trả 200 và
        // một thân rỗng. Coi đó là thành công thì cả thư viện được nạp bằng vector rỗng.
        None | Some(0) => EmbeddingProbeResult::fail(format!(
            "Máy chủ ở {url} nhận `{model}` nhưng không trả về vector nào. Đây thường là \
             một mô hình trò chuyện, không phải mô hình nhúng."
        )),
        Some(dimensions) => EmbeddingProbeResult {
            ok: true,
            message: format!("Nhúng được bằng `{model}`: vector {dimensions} chiều."),
            dimensions: Some(dimensions),
        },
    }
}

/// Gốc máy chủ cho `/api/embed`: giống [`crate::embed`], một đuôi `/v1` lạc vào đây thành
/// một URL không tồn tại.
fn embed_root(base_url: &str) -> String {
    let value = base_url.trim().trim_end_matches('/');
    let tail = value.rsplit('/').next().unwrap_or_default();
    if tail.starts_with('v') && tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit()) {
        value[..value.len() - tail.len()]
            .trim_end_matches('/')
            .to_string()
    } else {
        value.to_string()
    }
}

/// Một mã lỗi và một thân trả về thành câu nói phải làm gì.
///
/// Ba nhóm ở đây — sai khoá, mô hình không tồn tại, mô hình không nhúng được — chỉ phân
/// biệt được bằng cách đọc cả thân: Ollama trả 400 cho cả "chưa kéo mô hình về" lẫn "mô
/// hình này không có endpoint embed", và hai chuyện đó là hai việc phải làm khác hẳn nhau.
fn explain(config: &ProviderConfig, model: &str, url: &str, status: u16, body: &str) -> String {
    let err = pai_llm::LlmError::from_status(status, body);
    let lowered = body.to_lowercase();
    if matches!(err.code, LlmErrorCode::Auth) {
        return if config.api_key.is_empty() {
            format!("Máy chủ ở {url} đòi khoá API mà cấu hình này chưa có khoá.")
        } else {
            format!("Máy chủ ở {url} từ chối khoá API này (HTTP {status}). Kiểm tra lại khoá.")
        };
    }
    let missing = lowered.contains("not found")
        || lowered.contains("does not exist")
        || lowered.contains("unknown model")
        || lowered.contains("no such")
        || lowered.contains("try pulling");
    if missing {
        return match config.kind {
            ProviderKind::Ollama => format!(
                "Máy chủ ở {url} không có mô hình `{model}`. Kéo nó về bằng \
                 `ollama pull {model}` rồi thử lại."
            ),
            ProviderKind::OpenAiCompatible => format!(
                "Máy chủ ở {url} không biết mô hình `{model}`. Kiểm tra lại tên — mô hình \
                 nhúng có tên riêng, không dùng chung với mô hình trò chuyện."
            ),
        };
    }
    if lowered.contains("embed") || lowered.contains("not support") {
        return format!(
            "Máy chủ ở {url} có `{model}` nhưng không nhúng được bằng nó ({}). Chọn một mô \
             hình nhúng, ví dụ `{}`.",
            err.message,
            crate::embed::default_embedding_model(config.kind)
        );
    }
    format!(
        "Máy chủ ở {url} trả về lỗi khi nhúng bằng `{model}`: {}",
        err.message
    )
}

/// Số chiều của vector đầu tiên, theo hình dạng thân của từng bên.
fn first_vector(kind: ProviderKind, payload: &Value) -> Option<usize> {
    let row = match kind {
        ProviderKind::Ollama => payload.get("embeddings")?.as_array()?.first()?,
        ProviderKind::OpenAiCompatible => {
            payload.get("data")?.as_array()?.first()?.get("embedding")?
        }
    };
    Some(row.as_array()?.len())
}
