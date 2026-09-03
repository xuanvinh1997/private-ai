//! Kho mô hình LM Studio: `/api/v0/models` và `/api/v0/models/{id}`.
//!
//! Đây là `/api/show` của phía LM Studio, và nó trả lời được hai câu mà `/v1/models`
//! không trả lời được: mô hình này **làm được gì**, và nó **có đang nằm trong VRAM
//! không**. Cả hai đều đổi theo thời gian ở một máy chủ nạp theo yêu cầu, nên đoán theo
//! tên không phải một xấp xỉ tệ — nó là một câu trả lời sai.
//!
//! Nửa vòng đời thì **cố ý khuyết**, và khuyết một cách nói ra được. LM Studio không có
//! endpoint tải về, nhả, hay xoá: tải là việc của ứng dụng LM Studio (hoặc `lms get`),
//! nhả là việc của bộ đếm `ttl`, xoá là việc của người dùng trong thư mục mô hình. Ba
//! phương thức ấy trả [`LlmErrorCode::Unsupported`] kèm câu nói **phải làm gì thay thế**
//! — khác hẳn với việc trả `None` từ [`crate::seam::LlmAdapter::admin`], vì `None` sẽ
//! kéo mất cả `list` và `show`, tức là kéo mất chính thứ ta vừa xây.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use serde_json::Value;

use crate::capabilities::Capabilities;
use crate::error::{LlmError, LlmErrorCode};
use crate::model::{ModelDetails, ModelInfo, ModelState, PullProgress, RunningModel};
use crate::seam::ModelAdmin;

/// Bao lâu thì coi như máy chủ đã chết, khi chỉ hỏi thăm sức khoẻ.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub struct LmStudioAdmin {
    /// Gốc máy chủ, đã cắt mọi đuôi API — xem [`super::server_root`].
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl LmStudioAdmin {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, http: reqwest::Client) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            http,
        }
    }

    pub async fn health(&self) -> bool {
        self.authorized(self.http.get(format!("{}/api/v0/models", self.base_url)))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            return builder;
        }
        builder.bearer_auth(&self.api_key)
    }

    async fn get(&self, path: &str) -> Result<Value, LlmError> {
        let response = self
            .authorized(self.http.get(format!("{}{path}", self.base_url)))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::from_status(status.as_u16(), &body));
        }
        let text = response.text().await?;
        serde_json::from_str(&text).map_err(LlmError::from)
    }

    /// Một câu giải thích dùng chung cho ba động từ không có ở đây.
    fn khong_co(verb: &str, thay_the: &str) -> LlmError {
        LlmError::new(
            LlmErrorCode::Unsupported,
            format!(
                "LM Studio không có API để {verb} qua máy chủ cục bộ. {thay_the}"
            ),
        )
    }
}

/// Đọc một mục của `/api/v0/models` thành năng lực.
///
/// Ba nguồn, theo đúng thứ tự giảm dần độ tin cậy — và `source` phải nói đúng nguồn nào
/// đã thắng, vì "mô hình này không gọi được tool" là một câu khác hẳn khi nó đọc từ máy
/// chủ và khi nó chỉ là suy luận trên một cái tên:
///
/// 1. Mảng `capabilities` (bản LM Studio đủ mới), từ vựng riêng của họ.
/// 2. Cờ boolean rời — `trained_for_tool_use`, `vision` — các bản trung gian dùng cái này.
/// 3. `type`: `llm` / `vlm` / `embeddings`. Trường này có từ ngày đầu của `/api/v0` nên
///    trên thực tế nó luôn có mặt, và nó đủ để phân biệt mô hình nhúng với mô hình trò
///    chuyện — chuyện mà `/v1/models` không làm được và người dùng thì hay chọn nhầm.
///
/// Không nguồn nào trả lời được thì rơi xuống đoán theo tên cộng `arch`, y như nhánh dự
/// phòng của Ollama khi `/api/show` im lặng.
pub fn details_from_model(name: &str, entry: &Value) -> ModelDetails {
    let context_window = entry
        .get("loaded_context_length")
        .and_then(Value::as_u64)
        .or_else(|| entry.get("max_context_length").and_then(Value::as_u64));
    let arch = entry
        .get("arch")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut reported: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        let name = name.to_string();
        if !reported.contains(&name) {
            reported.push(name);
        }
    };

    // (1) Từ vựng riêng của LM Studio, dịch sang từ vựng chung của crate này.
    if let Some(items) = entry.get("capabilities").and_then(Value::as_array) {
        for item in items.iter().filter_map(Value::as_str) {
            match item.to_lowercase().as_str() {
                "tool_use" | "tools" | "function_calling" => push("tools"),
                "vision" | "image_input" => push("vision"),
                "embedding" | "embeddings" => push("embedding"),
                "thinking" | "reasoning" => push("thinking"),
                "chat" | "completion" => push("chat"),
                _ => {}
            }
        }
    }
    // (2) Cờ rời. Chỉ đọc khi nó là `true`: một `false` ở đây thường là trường chưa được
    // điền chứ không phải một lời khẳng định, và đọc nó thành "không có" là biến một chỗ
    // trống thành một cảnh báo sai.
    if entry
        .get("trained_for_tool_use")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        push("tools");
    }
    if entry.get("vision").and_then(Value::as_bool).unwrap_or(false) {
        push("vision");
    }
    // (3) Loại mô hình.
    match entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "embeddings" | "embedding" => push("embedding"),
        "vlm" => {
            push("chat");
            push("vision");
        }
        "llm" => push("chat"),
        _ => {}
    }

    let capabilities = Capabilities::from_reported(&reported, context_window).unwrap_or_else(|| {
        let mut inferred =
            Capabilities::infer(&format!("{name} {}", arch.clone().unwrap_or_default()));
        inferred.context_window = context_window;
        inferred
    });

    ModelDetails {
        capabilities,
        family: arch,
        // LM Studio không khai số tham số ở đâu cả; nó nằm trong tên tệp và ta không đoán.
        parameter_size: None,
        quantization: entry
            .get("quantization")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Mục này có đang nằm trong VRAM không.
fn loaded(entry: &Value) -> bool {
    entry
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| state.eq_ignore_ascii_case("loaded"))
}

fn info_from_entry(entry: &Value) -> Option<ModelInfo> {
    let name = entry.get("id").and_then(Value::as_str)?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let details = details_from_model(&name, entry);
    Some(ModelInfo {
        state: if loaded(entry) {
            ModelState::Loaded
        } else {
            ModelState::Unloaded
        },
        // LM Studio không khai kích thước tệp hay VRAM đang chiếm. Số 0 ở đây nghĩa là
        // **chưa biết**, đúng như tài liệu của trường nói, và nó không được đoán: một con
        // số bịa ra sẽ đi thẳng vào phép giữ chỗ VRAM của `required_bytes`.
        size_bytes: 0,
        vram_bytes: 0,
        quantization: details.quantization.clone(),
        modified_at: None,
        capabilities: details.capabilities,
        name,
    })
}

#[async_trait]
impl ModelAdmin for LmStudioAdmin {
    async fn list(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let payload = self.get("/api/v0/models").await?;
        let Some(entries) = payload.get("data").and_then(Value::as_array) else {
            return Err(LlmError::invalid(
                "LM Studio trả về danh sách mô hình không hợp lệ",
            ));
        };
        // Một lời gọi cho cả kho, không phải một lời gọi cho mỗi mô hình như Ollama phải
        // làm: `/api/v0/models` đã mang sẵn `type`, `state` và cửa sổ ngữ cảnh của từng
        // mục, nên không có gì để hỏi thêm.
        let mut models: Vec<ModelInfo> = entries.iter().filter_map(info_from_entry).collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }

    async fn running(&self) -> Result<Vec<RunningModel>, LlmError> {
        let payload = self.get("/api/v0/models").await?;
        let entries = payload
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(entries
            .iter()
            .filter(|entry| loaded(entry))
            .filter_map(|entry| {
                Some(RunningModel {
                    name: entry.get("id").and_then(Value::as_str)?.to_string(),
                    size_bytes: 0,
                    vram_bytes: 0,
                    // LM Studio nhả theo `ttl` của từng request chứ không theo một hạn
                    // chung cho cả mô hình, nên không có mốc nào để in ra đây.
                    expires_at: None,
                })
            })
            .collect())
    }

    async fn show(&self, model: &str) -> Result<ModelDetails, LlmError> {
        // Tên mô hình của LM Studio có dấu `/` (`lmstudio-community/qwen…`), nên nó phải
        // được mã hoá — nếu không thì đường dẫn tự mọc thêm một đoạn và máy chủ trả 404.
        let payload = self
            .get(&format!("/api/v0/models/{}", encode_segment(model)))
            .await?;
        Ok(details_from_model(model, &payload))
    }

    fn pull(&self, model: &str) -> BoxStream<'_, Result<PullProgress, LlmError>> {
        let err = Self::khong_co(
            "tải mô hình về",
            &format!(
                "Tải `{model}` trong chính ứng dụng LM Studio (tab Discover), hoặc bằng \
                 `lms get {model}`, rồi bấm làm mới ở đây."
            ),
        );
        Box::pin(stream::once(async move { Err(err) }))
    }

    async fn unload(&self, model: &str) -> Result<(), LlmError> {
        Err(Self::khong_co(
            "nhả mô hình khỏi VRAM",
            &format!(
                "LM Studio nhả theo bộ đếm `ttl` của chính nó; đặt thời gian giữ ấm trong \
                 tab Developer, hoặc chạy `lms unload {model}`."
            ),
        ))
    }

    async fn delete(&self, model: &str) -> Result<(), LlmError> {
        Err(Self::khong_co(
            "xoá mô hình khỏi đĩa",
            &format!("Xoá `{model}` trong tab My Models của LM Studio."),
        ))
    }
}

/// Mã hoá một đoạn đường dẫn. Viết tay vì đây là chỗ duy nhất cần tới, và một crate mã
/// hoá URL cho đúng một hàm là một phụ thuộc phải nuôi qua từng bản phát hành.
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
