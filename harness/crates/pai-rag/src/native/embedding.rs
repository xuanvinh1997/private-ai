use std::time::Duration;

use reqwest::Client;
use serde_json::{Value, json};

use super::config::ProviderConfig;
use crate::RagError;

pub const EMBED_INPUT_VERSION: u32 = 1;
pub const MAX_BATCH: usize = 64;

#[derive(Clone)]
pub struct EmbeddingClient {
    provider: ProviderConfig,
    query_prefix: &'static str,
    document_prefix: &'static str,
    http: Client,
}

impl EmbeddingClient {
    pub fn new(provider: ProviderConfig) -> Result<Self, RagError> {
        if provider.model.trim().is_empty() {
            return Err(RagError::Unavailable(
                "chưa chọn mô hình nhúng. Chọn một mô hình trong Cài đặt.".into(),
            ));
        }
        let (query_prefix, document_prefix) = prefixes_for(&provider.model);
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|error| RagError::Service(format!("không dựng được HTTP client: {error}")))?;
        Ok(Self {
            provider,
            query_prefix,
            document_prefix,
            http,
        })
    }

    pub fn model(&self) -> &str {
        &self.provider.model
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>, RagError> {
        let input = format!("{}{text}", self.query_prefix);
        let mut rows = self.embed_raw(&[input]).await?;
        rows.pop().ok_or_else(|| {
            RagError::Service(format!("model `{}` trả về vector rỗng", self.model()))
        })
    }

    pub async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        let prefixed: Vec<_> = texts
            .iter()
            .map(|text| format!("{}{text}", self.document_prefix))
            .collect();
        self.embed_raw(&prefixed).await
    }

    async fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        let mut output = Vec::with_capacity(texts.len());
        for batch in texts.chunks(MAX_BATCH) {
            let root = self.provider.root();
            let (url, field) = if self.provider.kind == "ollama" {
                (format!("{root}/api/embed"), "embeddings")
            } else {
                (format!("{root}/v1/embeddings"), "data")
            };
            let mut request = self.http.post(&url).json(&json!({
                "model": self.provider.model,
                "input": batch,
            }));
            if !self.provider.api_key.is_empty() {
                request = request.bearer_auth(&self.provider.api_key);
            }
            let response = request.send().await.map_err(|error| {
                RagError::Service(format!("không gọi được máy chủ nhúng ở {url}: {error}"))
            })?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                RagError::Service(format!("không đọc được phản hồi nhúng: {error}"))
            })?;
            if !status.is_success() {
                return Err(RagError::Service(format!(
                    "máy chủ nhúng trả {status} cho model `{}`: {}",
                    self.model(),
                    first_line(&body)
                )));
            }
            let payload: Value = serde_json::from_str(&body).map_err(|error| {
                RagError::Service(format!("phản hồi nhúng không phải JSON: {error}"))
            })?;
            let vectors = if field == "embeddings" {
                read_ollama(&payload, batch.len(), self.model())?
            } else {
                read_openai(&payload, batch.len(), self.model())?
            };
            output.extend(vectors);
        }
        Ok(output)
    }
}

pub fn prefixes_for(model: &str) -> (&'static str, &'static str) {
    let name = model.trim().to_ascii_lowercase();
    if name.contains("nomic-embed") {
        return ("search_query: ", "search_document: ");
    }
    if name == "e5"
        || name.contains("multilingual-e5")
        || name.contains("-e5-")
        || name.ends_with("-e5")
    {
        return ("query: ", "passage: ");
    }
    if name.contains("qwen3-embedding") || name.contains("qwen3_embedding") {
        return (
            "Instruct: Given a search query, retrieve relevant passages that answer it\nQuery: ",
            "",
        );
    }
    if name.contains("bge-")
        && !name.contains("bge-m3")
        && ["large", "base", "small"]
            .iter()
            .any(|size| name.contains(size))
    {
        return (
            "Represent this sentence for searching relevant passages: ",
            "",
        );
    }
    if name.contains("embeddinggemma") || name.contains("embedding-gemma") {
        return ("task: search result | query: ", "title: none | text: ");
    }
    ("", "")
}

fn read_ollama(payload: &Value, expected: usize, model: &str) -> Result<Vec<Vec<f32>>, RagError> {
    let rows = payload
        .get("embeddings")
        .and_then(Value::as_array)
        .ok_or_else(|| RagError::Service("phản hồi Ollama thiếu `embeddings`".into()))?;
    read_rows(rows.iter().collect(), expected, model)
}

fn read_openai(payload: &Value, expected: usize, model: &str) -> Result<Vec<Vec<f32>>, RagError> {
    let rows = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| RagError::Service("phản hồi nhúng thiếu `data`".into()))?;
    let mut ordered: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(fallback, row)| {
            (
                row.get("index")
                    .and_then(Value::as_u64)
                    .map_or(fallback, |index| index as usize),
                row.get("embedding").unwrap_or(&Value::Null),
            )
        })
        .collect();
    ordered.sort_by_key(|(index, _)| *index);
    read_rows(
        ordered.into_iter().map(|(_, row)| row).collect(),
        expected,
        model,
    )
}

fn read_rows(rows: Vec<&Value>, expected: usize, model: &str) -> Result<Vec<Vec<f32>>, RagError> {
    if rows.len() != expected {
        return Err(RagError::Service(format!(
            "model `{model}`: xin {expected} vector nhưng nhận {}",
            rows.len()
        )));
    }
    let vectors: Vec<Vec<f32>> = rows
        .into_iter()
        .map(|row| {
            row.as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_f64)
                        .map(|value| value as f32)
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    if vectors.iter().any(Vec::is_empty) {
        return Err(RagError::Service(format!(
            "model `{model}` trả về vector rỗng"
        )));
    }
    Ok(vectors)
}

fn first_line(body: &str) -> String {
    body.lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_prefixes_match_python_policy() {
        assert_eq!(
            prefixes_for("nomic-embed-text"),
            ("search_query: ", "search_document: ")
        );
        assert_eq!(prefixes_for("bge-m3"), ("", ""));
        assert!(
            prefixes_for("qwen3-embedding:4b")
                .0
                .starts_with("Instruct:")
        );
    }

    #[test]
    fn openai_rows_are_restored_by_index() {
        let payload = json!({"data": [
            {"index": 1, "embedding": [2.0]},
            {"index": 0, "embedding": [1.0]}
        ]});
        assert_eq!(
            read_openai(&payload, 2, "test").unwrap(),
            vec![vec![1.0], vec![2.0]]
        );
    }
}
