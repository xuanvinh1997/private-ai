use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde_json::{Value, json};

use super::config::VectorConfig;
use crate::RagError;

const FIELD_MODEL: &str = "_embed_model";
const FIELD_INPUT: &str = "_embed_input";

#[derive(Clone)]
pub struct Qdrant {
    base_url: String,
    api_key: String,
    collection: String,
    http: Client,
}

impl Qdrant {
    pub fn new(config: &VectorConfig, collection: String) -> Result<Self, RagError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| {
                RagError::Service(format!("không dựng được Qdrant client: {error}"))
            })?;
        Ok(Self {
            base_url: config.url.trim_end_matches('/').to_owned(),
            api_key: config.api_key.clone(),
            collection,
            http,
        })
    }

    pub async fn ensure(
        &self,
        dim: usize,
        model: &str,
        input_version: u32,
    ) -> Result<bool, RagError> {
        let existing = self.collection_info().await?;
        if let Some(info) = existing {
            if self.compatible(&info, dim, model, input_version).await? {
                return Ok(false);
            }
            self.drop_collection().await?;
        }

        self.expect_success(
            self.request(Method::PUT, &format!("/collections/{}", self.collection))
                .json(&json!({"vectors": {"size": dim, "distance": "Cosine"}}))
                .send()
                .await,
            "tạo collection Qdrant",
        )
        .await?;
        self.expect_success(
            self.request(
                Method::PUT,
                &format!("/collections/{}/index", self.collection),
            )
            .query(&[("wait", "true")])
            .json(&json!({"field_name": "document_id", "field_schema": "keyword"}))
            .send()
            .await,
            "tạo chỉ mục payload Qdrant",
        )
        .await?;
        Ok(true)
    }

    async fn compatible(
        &self,
        info: &Value,
        dim: usize,
        model: &str,
        input_version: u32,
    ) -> Result<bool, RagError> {
        let found_dim = info
            .pointer("/result/config/params/vectors/size")
            .and_then(Value::as_u64);
        if found_dim.is_some_and(|found| found as usize != dim) {
            return Ok(false);
        }
        let payload = self.sample_payload().await?;
        let Some(payload) = payload else {
            return Ok(true);
        };
        Ok(
            payload.get(FIELD_MODEL).and_then(Value::as_str) == Some(model)
                && payload.get(FIELD_INPUT).and_then(Value::as_u64)
                    == Some(u64::from(input_version)),
        )
    }

    pub async fn existing_ids(&self, ids: &[i64]) -> Result<Vec<i64>, RagError> {
        let mut output = Vec::new();
        for batch in ids.chunks(1_000) {
            let response = self
                .request(
                    Method::POST,
                    &format!("/collections/{}/points", self.collection),
                )
                .json(&json!({"ids": batch, "with_payload": false, "with_vector": false}))
                .send()
                .await;
            let payload = self.json(response, "đọc mã điểm Qdrant").await?;
            if let Some(rows) = payload.get("result").and_then(Value::as_array) {
                output.extend(
                    rows.iter()
                        .filter_map(|row| row.get("id").and_then(Value::as_i64)),
                );
            }
        }
        Ok(output)
    }

    pub async fn upsert(
        &self,
        ids: &[i64],
        vectors: &[Vec<f32>],
        payloads: &[Value],
        model: &str,
        input_version: u32,
    ) -> Result<(), RagError> {
        if ids.len() != vectors.len() || ids.len() != payloads.len() {
            return Err(RagError::Service("lệch số mã, vector và payload".into()));
        }
        let points: Vec<_> = ids
            .iter()
            .zip(vectors)
            .zip(payloads)
            .map(|((&id, vector), payload)| {
                let mut payload = payload.as_object().cloned().unwrap_or_default();
                payload.insert(FIELD_MODEL.into(), Value::String(model.to_owned()));
                payload.insert(FIELD_INPUT.into(), json!(input_version));
                json!({"id": id, "vector": vector, "payload": payload})
            })
            .collect();
        self.expect_success(
            self.request(
                Method::PUT,
                &format!("/collections/{}/points", self.collection),
            )
            .query(&[("wait", "true")])
            .json(&json!({"points": points}))
            .send()
            .await,
            "ghi vector vào Qdrant",
        )
        .await
    }

    pub async fn search(&self, vector: &[f32], limit: usize) -> Result<Vec<i64>, RagError> {
        let response = self
            .request(
                Method::POST,
                &format!("/collections/{}/points/query", self.collection),
            )
            .json(&json!({"query": vector, "limit": limit, "with_payload": false}))
            .send()
            .await;
        let payload = self.json(response, "tìm vector trong Qdrant").await?;
        Ok(payload
            .pointer("/result/points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|row| row.get("id").and_then(Value::as_i64))
            .collect())
    }

    pub async fn count(&self) -> Result<usize, RagError> {
        if self.collection_info().await?.is_none() {
            return Ok(0);
        }
        let response = self
            .request(
                Method::POST,
                &format!("/collections/{}/points/count", self.collection),
            )
            .json(&json!({"exact": true}))
            .send()
            .await;
        let payload = self.json(response, "đếm vector Qdrant").await?;
        payload
            .pointer("/result/count")
            .and_then(Value::as_u64)
            .map(|count| count as usize)
            .ok_or_else(|| RagError::Service("Qdrant không trả về `result.count`".into()))
    }

    pub async fn remove_document(&self, document_id: &str) -> Result<(), RagError> {
        if self.collection_info().await?.is_none() {
            return Ok(());
        }
        self.expect_success(
            self.request(
                Method::POST,
                &format!("/collections/{}/points/delete", self.collection),
            )
            .query(&[("wait", "true")])
            .json(&json!({
                "filter": {"must": [{"key": "document_id", "match": {"value": document_id}}]}
            }))
            .send()
            .await,
            "xoá vector của tài liệu",
        )
        .await
    }

    pub async fn drop_collection(&self) -> Result<(), RagError> {
        let response = self
            .request(Method::DELETE, &format!("/collections/{}", self.collection))
            .send()
            .await;
        match response {
            Ok(response) if response.status() == StatusCode::NOT_FOUND => Ok(()),
            other => self.expect_success(other, "xoá collection Qdrant").await,
        }
    }

    async fn collection_info(&self) -> Result<Option<Value>, RagError> {
        let response = self
            .request(Method::GET, &format!("/collections/{}", self.collection))
            .send()
            .await
            .map_err(|error| RagError::Service(format!("không nối được Qdrant: {error}")))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(
            self.json(Ok(response), "đọc collection Qdrant").await?,
        ))
    }

    async fn sample_payload(&self) -> Result<Option<Value>, RagError> {
        let response = self
            .request(
                Method::POST,
                &format!("/collections/{}/points/scroll", self.collection),
            )
            .json(&json!({"limit": 1, "with_payload": true, "with_vector": false}))
            .send()
            .await;
        let payload = self.json(response, "đọc metadata vector Qdrant").await?;
        Ok(payload
            .pointer("/result/points/0/payload")
            .cloned()
            .filter(Value::is_object))
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base_url));
        if !self.api_key.is_empty() {
            request = request.header("api-key", &self.api_key);
        }
        request
    }

    async fn json(
        &self,
        response: Result<Response, reqwest::Error>,
        action: &str,
    ) -> Result<Value, RagError> {
        let response =
            response.map_err(|error| RagError::Service(format!("không thể {action}: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| RagError::Service(format!("không đọc được Qdrant: {error}")))?;
        if !status.is_success() {
            return Err(RagError::Service(format!(
                "không thể {action}: Qdrant trả {status}: {}",
                body.lines().next().unwrap_or_default()
            )));
        }
        serde_json::from_str(&body)
            .map_err(|error| RagError::Service(format!("Qdrant trả JSON hỏng: {error}")))
    }

    async fn expect_success(
        &self,
        response: Result<Response, reqwest::Error>,
        action: &str,
    ) -> Result<(), RagError> {
        self.json(response, action).await.map(|_| ())
    }
}
