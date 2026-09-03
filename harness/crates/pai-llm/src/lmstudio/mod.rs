//! Adapter LM Studio: hội thoại qua `/v1/chat/completions`, kho mô hình qua `/api/v0`.
//!
//! LM Studio **là** một máy chủ OpenAI-compatible, nên trước bản này nó chỉ là một mục
//! trong danh mục trỏ vào [`crate::openai::OpenAiAdapter`]. Điều đó chạy được, và nó bỏ
//! mất đúng nửa quan trọng nhất của một ứng dụng chạy mô hình tại chỗ — cùng nửa mà
//! [`crate::ollama::OllamaAdmin`] có:
//!
//! - `/v1/models` chỉ trả `id` và `owned_by`, nên mọi năng lực đều là **đoán theo tên**.
//!   Màn hình chọn mô hình vì thế dán nhãn "không gọi được công cụ" lên mọi mô hình của
//!   LM Studio, kể cả những mô hình gọi tool tốt — một cảnh báo sai ở đúng chỗ người dùng
//!   đang quyết định.
//! - Không có cách nào biết mô hình nào đang nằm trong VRAM, mà LM Studio thì nạp theo
//!   yêu cầu (JIT) nên trạng thái ấy đổi liên tục.
//!
//! `/api/v0/models` trả lời cả hai: `type`, `arch`, `quantization`, `state`,
//! `max_context_length`, và — với bản LM Studio đủ mới — cả năng lực tool/vision. Đây là
//! `/api/show` của phía LM Studio, và adapter này tồn tại để dùng nó.
//!
//! **Phần hội thoại vẫn là dây của OpenAI**, không viết lại: LM Studio nói đúng giao thức
//! ấy, và một bản cài đặt SSE thứ hai là một chỗ nữa để lệch. Chỉ có hai khác biệt được
//! thêm vào, cả hai đều nằm ở [`encode_chat`]: đường đi tới `/v1`, và `ttl` — bản dịch
//! của `keep_alive` sang từ vựng LM Studio.

pub mod admin;

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::BoxStream;
use serde_json::{Value, json};

use crate::capabilities::Capabilities;
use crate::error::LlmError;
use crate::message::ChatRequest;
use crate::seam::{LlmAdapter, ModelAdmin};
use crate::stream::StreamChunk;
use crate::wire::pump::pump;

pub use admin::LmStudioAdmin;

/// Nói chuyện với một máy chủ LM Studio.
pub struct LmStudioAdapter {
    id: String,
    /// **Gốc máy chủ**, không có đuôi `/v1` hay `/api/v0`.
    ///
    /// Adapter này nói hai giao thức trên cùng một host — `/v1` cho hội thoại và
    /// `/api/v0` cho kho mô hình — nên nó phải giữ gốc và tự nối đuôi, đúng như
    /// [`crate::ollama::OllamaAdapter`] giữ gốc rồi nối `/api`.
    base_url: String,
    api_key: String,
    http: reqwest::Client,
    admin: Arc<LmStudioAdmin>,
}

impl LmStudioAdapter {
    /// `base_url` nhận cả `http://localhost:1234`, `.../v1` lẫn `.../api/v0`.
    ///
    /// Ba dạng vì cả ba đều là thứ người dùng đang có trong tay: dạng giữa là cái LM
    /// Studio in ra trong tab Developer, dạng cuối là cái tài liệu REST của họ dùng, và
    /// dạng đầu là cái người ta gõ khi không chắc. Chuẩn hoá về gốc ở đúng một chỗ rẻ hơn
    /// nhiều so với một lỗi 404 mà người dùng phải tự đoán ra là do thừa một đuôi.
    pub fn new(
        id: impl Into<String>,
        base_url: impl AsRef<str>,
        api_key: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        let base_url = server_root(base_url.as_ref());
        let api_key = api_key.into();
        let admin = Arc::new(LmStudioAdmin::new(
            base_url.clone(),
            api_key.clone(),
            http.clone(),
        ));
        Self {
            id: id.into(),
            base_url,
            api_key,
            http,
            admin,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            // LM Studio cục bộ không đòi khoá, và một header `Bearer` giả chỉ là một chuỗi
            // vô nghĩa nữa lọt vào log. Người dùng đặt khoá thì tôn trọng — LM Studio bật
            // được xác thực khi phục vụ qua mạng nội bộ.
            return builder;
        }
        builder.bearer_auth(&self.api_key)
    }
}

/// Cắt mọi đuôi API quen thuộc để còn lại gốc máy chủ.
///
/// Cắt lặp chứ không cắt một lần: `http://host/api/v0` là **hai** đoạn phải bỏ, và một
/// người dán `http://host/api/v0/v1` (có thật, khi ghép hai đoạn tài liệu) vẫn phải ra
/// đúng gốc.
pub fn server_root(base_url: &str) -> String {
    let mut value = base_url.trim().trim_end_matches('/');
    loop {
        let tail = value.rsplit('/').next().unwrap_or_default();
        let versioned =
            tail.starts_with('v') && tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit());
        if !(versioned || tail == "api") {
            break;
        }
        value = value[..value.len() - tail.len()].trim_end_matches('/');
    }
    value.to_string()
}

#[async_trait]
impl LlmAdapter for LmStudioAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        let body = encode_chat(&req);
        let builder = self.authorized(
            self.http
                .post(format!("{}/v1/chat/completions", self.base_url)),
        );
        let request =
            async move { builder.json(&body).send().await.map_err(LlmError::from) }.boxed();
        // Cùng bộ giải mã SSE với OpenAI, không phải một bản sao: LM Studio phát đúng
        // hình dạng ấy, kể cả `reasoning_content` của các mô hình có kênh suy luận.
        pump(request, crate::openai::ChatDecoder::new())
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        // Cùng thứ tự bắt buộc như Ollama: hỏi máy chủ trước, đoán theo tên sau. Đây là
        // toàn bộ lý do adapter này tồn tại tách khỏi `OpenAiAdapter`.
        Ok(self
            .admin
            .show(model)
            .await
            .map(|details| details.capabilities)
            .unwrap_or_else(|_| Capabilities::infer(model)))
    }

    async fn health(&self) -> bool {
        self.admin.health().await
    }

    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        Some(self.admin.clone())
    }
}

/// Thân request `/v1/chat/completions`, cộng `ttl` của LM Studio.
///
/// Dựng trên [`crate::openai::encode_chat`] thay vì viết lại: mọi quyết định khó ở đó —
/// `content: null` khi có tool call, chuỗi thuần thay vì mảng khi không có ảnh — đều đúng
/// y nguyên ở đây, và chép chúng sang là chép cả những lần sửa sau này.
pub(crate) fn encode_chat(req: &ChatRequest) -> Value {
    let mut body = crate::openai::encode_chat(req);
    // `keep_alive` là từ vựng Ollama; LM Studio gọi cùng khái niệm ấy là `ttl` và đo bằng
    // **giây**. Adapter OpenAI bỏ qua trường này vì nó không có chỗ nào để đặt; ở đây thì
    // có, nên dịch chứ không bỏ — người dùng đặt "nhả ngay sau lượt" là đang nói một câu
    // có nghĩa với LM Studio y như với Ollama.
    if let Some(seconds) = req.keep_alive.as_deref().and_then(keep_alive_seconds)
        && let Some(object) = body.as_object_mut()
    {
        object.insert("ttl".into(), json!(seconds));
    }
    body
}

/// `"5m"`, `"30s"`, `"1h"`, `"0"`, `"300"` → số giây. `None` khi không đọc được.
///
/// Không báo lỗi cho chuỗi lạ: `keep_alive` là một **gợi ý về vòng đời**, và làm hỏng cả
/// lượt vì một hậu tố không nhận ra là đổi một tiện ích thành một cái bẫy.
fn keep_alive_seconds(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (number, factor) = match value.chars().last()? {
        's' => (&value[..value.len() - 1], 1),
        'm' => (&value[..value.len() - 1], 60),
        'h' => (&value[..value.len() - 1], 3600),
        _ => (value, 1),
    };
    number.trim().parse::<u64>().ok().map(|n| n * factor)
}
