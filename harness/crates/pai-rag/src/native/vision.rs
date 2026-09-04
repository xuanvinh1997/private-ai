use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Client;
use serde_json::{Value, json};

use super::config::ProviderConfig;
use crate::RagError;

const PROMPT: &str = "Trích xuất toàn bộ chữ nhìn thấy được trong ảnh này. Giữ nguyên tiêu đề, danh sách và bảng ở dạng Markdown. Không tóm tắt, không dịch, không thêm bất cứ chữ nào không có trong ảnh. Nếu ảnh không có chữ nào, trả về đúng một dòng trống.";

#[derive(Clone)]
pub struct VisionClient {
    provider: ProviderConfig,
    http: Client,
}

impl VisionClient {
    pub fn new(provider: ProviderConfig) -> Result<Self, RagError> {
        if provider.model.trim().is_empty() {
            return Err(RagError::Unavailable(
                "chưa chọn mô hình vision. Chọn một mô hình đọc ảnh trong Cài đặt.".into(),
            ));
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|error| {
                RagError::Service(format!("không dựng được vision client: {error}"))
            })?;
        Ok(Self { provider, http })
    }

    pub fn model(&self) -> &str {
        &self.provider.model
    }

    pub async fn ocr(&self, image: &[u8], mime: &str) -> Result<String, RagError> {
        let url = format!("{}/v1/chat/completions", self.provider.root());
        let mut request = self.http.post(&url).json(&json!({
            "model": self.provider.model,
            "max_tokens": 4096,
            "temperature": 0,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": PROMPT},
                    {"type": "image_url", "image_url": {
                        "url": format!("data:{mime};base64,{}", STANDARD.encode(image))
                    }}
                ]
            }]
        }));
        if !self.provider.api_key.is_empty() {
            request = request.bearer_auth(&self.provider.api_key);
        }
        let response = request.send().await.map_err(|error| {
            RagError::Extraction(format!("không gọi được model vision ở {url}: {error}"))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            RagError::Extraction(format!("không đọc được phản hồi vision: {error}"))
        })?;
        if !status.is_success() {
            return Err(RagError::Extraction(format!(
                "model vision `{}` trả {status}: {}",
                self.model(),
                body.lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect::<String>()
            )));
        }
        let payload: Value = serde_json::from_str(&body).map_err(|error| {
            RagError::Extraction(format!("phản hồi vision không phải JSON: {error}"))
        })?;
        let content = payload
            .pointer("/choices/0/message/content")
            .ok_or_else(|| RagError::Extraction("phản hồi vision không có `choices`".into()))?;
        Ok(read_content(content).trim().to_owned())
    }
}

fn read_content(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_openai_content_shapes() {
        assert_eq!(read_content(&json!("abc")), "abc");
        assert_eq!(
            read_content(&json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}])),
            "a\nb"
        );
    }
}
