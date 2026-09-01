//! Seam bộ nhúng, và hai bản cài đặt.
//!
//! Bộ nhúng là thứ **được phép vắng mặt**. Cả tầng trên được viết quanh giả định đó: khi
//! không có ai cắm vào seam này, hoặc khi máy chủ bên kia không trả lời, việc nạp vẫn
//! chạy tới cùng và tìm kiếm lùi về FTS5. Đó là lý do trait này trả `Result` chứ không
//! panic, và là lý do có [`Embedder::health`] — giao diện cần nói được **vì sao** phần
//! ngữ nghĩa chưa sẵn sàng, chứ không chỉ rằng nó chưa sẵn sàng.
//!
//! Cả hai bản cài đặt gọi HTTP trực tiếp bằng `reqwest`, cùng lối với `pai-llm`: hai
//! endpoint, mỗi cái một hình dạng thân request, và một thư viện client ở giữa chỉ thêm
//! một tầng phải đọc khi nó đoán sai.

use std::time::Duration;

use async_trait::async_trait;
use pai_core::ServiceKey;
use serde_json::{Value, json};

use crate::error::RagError;

/// Trần số phần tử một lô.
///
/// Có trần vì cả hai máy chủ đều có giới hạn kích thước thân request, và vì một lô lớn
/// hỏng là mất toàn bộ công của lô đó. 64 đoạn ~1000 ký tự là khoảng 64 KB — đủ lớn để
/// không phải trả phí thiết lập kết nối cho từng đoạn, đủ nhỏ để một lần hỏng chỉ mất vài
/// giây.
pub const MAX_BATCH: usize = 64;

/// Bao lâu thì coi như máy chủ đã chết, khi chỉ hỏi thăm sức khoẻ.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Một lần nhúng cả lô có thể chạy lâu: mô hình phải nạp vào VRAM ở lần gọi đầu tiên.
const EMBED_TIMEOUT: Duration = Duration::from_secs(120);

#[async_trait]
pub trait Embedder: Send + Sync {
    /// Tên để hiện lên giao diện và ghi vào `stats().embedder`.
    fn id(&self) -> &str;

    /// Số chiều, nếu biết trước. `None` là hợp lệ: nhiều máy chủ chỉ nói ra số chiều
    /// bằng cách trả về một vector, và bắt tầng trên phải biết trước thì nó phải gọi thử
    /// một lần chỉ để hỏi.
    fn dim(&self) -> Option<usize>;

    /// Nhúng một lô. Kết quả **cùng thứ tự và cùng độ dài** với đầu vào — tầng trên ghép
    /// vector với đoạn theo chỉ số, nên một máy chủ trả thiếu một phần tử phải là lỗi chứ
    /// không phải một sự lệch âm thầm.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError>;

    async fn health(&self) -> bool;
}

/// Seam bộ nhúng. Không có provider = tìm kiếm chỉ có từ khoá.
pub enum Embeddings {}
impl ServiceKey for Embeddings {
    type Api = dyn Embedder;
    const NAME: &'static str = "rag.embeddings";
}

/// Ollama: `POST {base}/api/embed`, thân `{"model": …, "input": [...]}`, đọc `embeddings`.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    dim: Option<usize>,
    http: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> OllamaEmbedder {
        OllamaEmbedder {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            dim: None,
            http: reqwest::Client::new(),
        }
    }

    /// Khai trước số chiều khi cấu hình biết nó. Không bắt buộc — xem [`Embedder::dim`].
    pub fn with_dim(mut self, dim: usize) -> OllamaEmbedder {
        self.dim = Some(dim);
        self
    }

    pub fn with_client(mut self, http: reqwest::Client) -> OllamaEmbedder {
        self.http = http;
        self
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn id(&self) -> &str {
        &self.model
    }

    fn dim(&self) -> Option<usize> {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(MAX_BATCH) {
            let body = json!({ "model": self.model, "input": batch });
            let payload = post_json(
                &self.http,
                self.id(),
                &format!("{}/api/embed", self.base_url),
                body,
                None,
            )
            .await?;
            out.extend(read_vectors(
                self.id(),
                &payload,
                "embeddings",
                batch.len(),
            )?);
        }
        Ok(out)
    }

    async fn health(&self) -> bool {
        self.http
            .get(format!("{}/api/tags", self.base_url))
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }
}

/// OpenAI và mọi máy chủ nói cùng giao thức: `POST {base}/v1/embeddings`, có `Bearer`.
pub struct OpenAiEmbedder {
    base_url: String,
    model: String,
    api_key: String,
    dim: Option<usize>,
    http: reqwest::Client,
}

impl OpenAiEmbedder {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> OpenAiEmbedder {
        OpenAiEmbedder {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key: api_key.into(),
            dim: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_dim(mut self, dim: usize) -> OpenAiEmbedder {
        self.dim = Some(dim);
        self
    }

    pub fn with_client(mut self, http: reqwest::Client) -> OpenAiEmbedder {
        self.http = http;
        self
    }
}

#[async_trait]
impl Embedder for OpenAiEmbedder {
    fn id(&self) -> &str {
        &self.model
    }

    fn dim(&self) -> Option<usize> {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(MAX_BATCH) {
            let body = json!({ "model": self.model, "input": batch });
            let payload = post_json(
                &self.http,
                self.id(),
                &format!("{}/v1/embeddings", self.base_url),
                body,
                Some(&self.api_key),
            )
            .await?;
            out.extend(read_openai_vectors(self.id(), &payload, batch.len())?);
        }
        Ok(out)
    }

    async fn health(&self) -> bool {
        self.http
            .get(format!("{}/v1/models", self.base_url))
            .bearer_auth(&self.api_key)
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }
}

/// Một hàm POST dùng chung cho cả hai. Chúng khác nhau đúng một header và một hình dạng
/// thân trả về; hai bản sao của cùng đoạn xử lý lỗi là hai chỗ để thông báo lỗi trôi ra
/// khỏi nhau qua thời gian.
async fn post_json(
    http: &reqwest::Client,
    id: &str,
    url: &str,
    body: Value,
    key: Option<&str>,
) -> Result<Value, RagError> {
    let fail = |reason: String| RagError::Embed {
        id: id.to_string(),
        reason,
    };
    let mut request = http.post(url).timeout(EMBED_TIMEOUT).json(&body);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|err| fail(err.to_string()))?;
    let status = response.status();
    let text = response.text().await.map_err(|err| fail(err.to_string()))?;
    if !status.is_success() {
        // Kèm thân trả về: một `401` trơ trọi không nói được đây là khoá sai hay hết hạn
        // mức, mà đó lại là hai việc phải làm khác hẳn nhau.
        return Err(fail(format!("máy chủ trả {status}: {}", first_line(&text))));
    }
    serde_json::from_str(&text).map_err(|err| fail(format!("phản hồi không phải JSON: {err}")))
}

/// `{"embeddings": [[…], […]]}` của Ollama.
fn read_vectors(
    id: &str,
    payload: &Value,
    field: &str,
    expected: usize,
) -> Result<Vec<Vec<f32>>, RagError> {
    let rows = payload
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| RagError::Embed {
            id: id.to_string(),
            reason: format!("phản hồi thiếu trường `{field}`"),
        })?;
    let vectors: Vec<Vec<f32>> = rows.iter().map(read_row).collect();
    expect_len(id, vectors.len(), expected)?;
    Ok(vectors)
}

/// `{"data": [{"index": 0, "embedding": […]}]}` của OpenAI.
///
/// Sắp lại theo `index` chứ không tin thứ tự trong mảng: spec cho phép trả về không theo
/// thứ tự, và một lần lệch ở đây gán vector của đoạn này cho đoạn kia — một lỗi không
/// bao giờ báo, chỉ làm kết quả tìm kiếm sai một cách khó hiểu.
fn read_openai_vectors(
    id: &str,
    payload: &Value,
    expected: usize,
) -> Result<Vec<Vec<f32>>, RagError> {
    let rows = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| RagError::Embed {
            id: id.to_string(),
            reason: "phản hồi thiếu trường `data`".to_string(),
        })?;
    let mut indexed: Vec<(usize, Vec<f32>)> = rows
        .iter()
        .enumerate()
        .map(|(fallback, row)| {
            let index = row
                .get("index")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(fallback);
            (
                index,
                read_row(row.get("embedding").unwrap_or(&Value::Null)),
            )
        })
        .collect();
    indexed.sort_by_key(|(index, _)| *index);
    expect_len(id, indexed.len(), expected)?;
    Ok(indexed.into_iter().map(|(_, vector)| vector).collect())
}

fn read_row(row: &Value) -> Vec<f32> {
    row.as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_f64)
                .map(|value| value as f32)
                .collect()
        })
        .unwrap_or_default()
}

fn expect_len(id: &str, got: usize, expected: usize) -> Result<(), RagError> {
    if got == expected {
        return Ok(());
    }
    Err(RagError::Embed {
        id: id.to_string(),
        reason: format!("xin {expected} vector nhưng nhận {got}"),
    })
}

/// Thân lỗi của một máy chủ có thể là cả một trang HTML. Một dòng là đủ để đọc.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > 200 {
        line.chars().take(200).collect::<String>() + "…"
    } else {
        line.to_string()
    }
}
