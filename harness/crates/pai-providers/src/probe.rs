//! Try a configuration before saving it, with someone waiting on a button: short timeouts, and three
//! distinct messages for the three situations, since each demands a different action.
//! [`probe_embedding`] differs -- it actually embeds a sentence rather than listing models.

use std::time::Duration;

use pai_llm::{Capabilities, LlmErrorCode, ProviderConfig, ProviderKind, openai_base_url};
use serde_json::Value;

/// How long a user waits before assuming the app has hung.
const TIMEOUT: Duration = Duration::from_secs(4);

/// A model the server claims to have.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeModel {
    pub id: String,
    /// Guessed from the name, not verified: a probe will not pay for `/api/show` per model. Authority comes later.
    pub tools: bool,
    /// Chat-capable, with the same confidence as [`ProbeModel::tools`].
    pub chat: bool,
    /// Embedding-capable: a guess on Ollama and OpenAI-compatible, but a useful one for sorting the picker,
    /// where a wrong order costs a scroll. On LM Studio it is authoritative, from `type: "embeddings"`.
    pub embedding: bool,
    pub context_window: Option<u64>,
}

/// The result of one probe.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeResult {
    pub ok: bool,
    /// One sentence saying what to do next, not what went wrong.
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

/// Call a config's model-listing endpoint, bypassing [`pai_llm::AdapterRegistry`] so an unsaved, mistyped URL
/// does not linger in the adapter cache after the dialog is dismissed.
pub async fn probe(config: &ProviderConfig, http: &reqwest::Client) -> ProbeResult {
    let (url, authorized) = match config.kind {
        ProviderKind::Ollama => (
            format!("{}/api/tags", config.base_url.trim_end_matches('/')),
            false,
        ),
        // `/api/v0/models`, not `/v1/models`: same server, but the latter returns only ids.
        ProviderKind::LmStudio => (
            format!("{}/api/v0/models", pai_llm::lmstudio::server_root(&config.base_url)),
            !config.api_key.is_empty(),
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
            // Not-connected group: nothing is listening yet, and the key was never asked about, so do not mention it.
            let detail = if err.is_timeout() {
                "máy chủ không trả lời kịp"
            } else {
                "không mở được kết nối"
            };
            return ProbeResult::fail(format!(
                "Không kết nối được tới {url} ({detail}). Kiểm tra máy chủ đã chạy chưa và địa \
                 chỉ có đúng không."
            ));
        }
    };

    let status = response.status().as_u16();
    if !status_ok(status) {
        let body = response.text().await.unwrap_or_default();
        let err = pai_llm::LlmError::from_status(status, &body);
        return ProbeResult::fail(match err.code {
            // Bad-key group: the server is there, talking, and refusing us.
            LlmErrorCode::Auth if config.api_key.is_empty() => {
                format!("API Key invalid!")
            }
            LlmErrorCode::Auth => {
                format!("API Key invalid! Check your key and try again.")
            }
            _ => format!("Máy chủ ở {url} trả về lỗi HTTP {status}: {}", err.message),
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
        ProviderKind::LmStudio => parse_lmstudio(&payload),
        ProviderKind::OpenAiCompatible => parse_names(&payload, "data", "id"),
    };

    if models.is_empty() {
        // Empty group: connected and authenticated but nothing to run, so the fix is pulling a model, not editing config.
        return ProbeResult::fail(match config.kind {
            ProviderKind::Ollama => format!(
                "Kết nối được tới {url}, nhưng máy chủ chưa có mô hình nào. Tải một mô hình về \
                 bằng `ollama pull` trước đã."
            ),
            ProviderKind::LmStudio => format!(
                "Kết nối được tới {url}, nhưng LM Studio chưa có mô hình nào. Tải một mô hình ở \
                 tab Discover rồi thử lại."
            ),
            ProviderKind::OpenAiCompatible => format!(
                "Kết nối được tới {url}, nhưng máy chủ không trả về mô hình nào. Với một máy chủ \
                 tự dựng thì đó thường là do chưa nạp mô hình lúc khởi động."
            ),
        });
    }

    ProbeResult {
        ok: true,
        message: format!("Kết nối được. Máy chủ có {} mô hình.", models.len()),
        models,
    }
}

fn status_ok(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Read LM Studio's `/api/v0/models`, the one place where `tools` is authoritative: the listing already
/// declares model type and capabilities, with no per-model call.
fn parse_lmstudio(payload: &Value) -> Vec<ProbeModel> {
    payload
        .get("data")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id").and_then(Value::as_str)?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    let caps = pai_llm::lmstudio::admin::details_from_model(id, entry).capabilities;
                    Some(ProbeModel {
                        id: id.to_string(),
                        tools: caps.tools,
                        chat: caps.chat,
                        embedding: caps.embedding,
                        context_window: caps.context_window,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
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
                        chat: caps.chat,
                        embedding: caps.embedding,
                        context_window: caps.context_window,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Budget for one embedding probe; longer than [`TIMEOUT`] because a local server's first embed also loads the model into VRAM.
const EMBED_TIMEOUT: Duration = Duration::from_secs(8);

/// The sample sentence to embed; accented Vietnamese on purpose, since a mis-tokenised server swallows `hello` but fails here.
const EMBED_SAMPLE: &str = "Một câu tiếng Việt ngắn để thử.";

/// The result of one real embedding probe.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingProbeResult {
    pub ok: bool,
    /// One sentence saying what to do next.
    pub message: String,
    /// Dimensions measured from the vector actually returned.
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

/// Embed a sentence and measure the returned vector. Listing cannot answer the real question -- Ollama's
/// `/api/tags` says nothing about which models embed -- so the only certainty is sending text and looking
/// for a vector. Four situations, four messages, because each demands a different action.
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
        // For embedding, LM Studio is simply an OpenAI server: `/v1/embeddings`, identical body.
        ProviderKind::LmStudio | ProviderKind::OpenAiCompatible => {
            match openai_base_url(&config.base_url) {
                Ok(root) => (format!("{root}/embeddings"), !config.api_key.is_empty()),
                Err(err) => return EmbeddingProbeResult::fail(err.message),
            }
        }
    };

    // A client built on the spot: the config may never have been saved, and one button press is not worth threading a client through.
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
            // Not-connected group, same rule as [`probe`]: the key was never asked about, so do not mention it.
            let detail = if err.is_timeout() {
                "máy chủ không trả lời kịp"
            } else {
                "không mở được kết nối"
            };
            return EmbeddingProbeResult::fail(format!(
                "Không kết nối được tới {url} ({detail}). Kiểm tra máy chủ đã chạy chưa và địa \
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
        // Exists-but-cannot-embed in its nastiest form: HTTP 200 with an empty body, which if trusted indexes the whole library as empty vectors.
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

/// Host root for `/api/embed`; as in [`crate::embed`], a stray `/v1` suffix would make a nonexistent URL.
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

/// Turn a status and body into a next-action sentence; bad key, missing model and non-embedding model are
/// only distinguishable by reading the body, since Ollama returns 400 for the last two alike.
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
            ProviderKind::LmStudio => format!(
                "LM Studio ở {url} không có mô hình `{model}`. Tải một mô hình nhúng ở tab \
                 Discover — nó có tên riêng, không dùng chung với mô hình trò chuyện."
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

/// Dimensions of the first vector, per each side's body shape.
fn first_vector(kind: ProviderKind, payload: &Value) -> Option<usize> {
    let row = match kind {
        ProviderKind::Ollama => payload.get("embeddings")?.as_array()?.first()?,
        ProviderKind::LmStudio | ProviderKind::OpenAiCompatible => {
            payload.get("data")?.as_array()?.first()?.get("embedding")?
        }
    };
    Some(row.as_array()?.len())
}
