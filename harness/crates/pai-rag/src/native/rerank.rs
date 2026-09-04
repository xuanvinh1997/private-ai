use serde::{Deserialize, Serialize};

use super::config::RerankConfig;
use crate::RagError;

#[derive(Debug, Deserialize)]
pub struct Scored {
    pub index: usize,
    #[serde(rename = "relevance_score", alias = "score")]
    pub score: f32,
}

#[derive(Serialize)]
struct Request<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    query: &'a str,
    documents: &'a [&'a str],
}

#[derive(Deserialize)]
struct Response {
    results: Vec<Scored>,
}

pub async fn http(
    config: &RerankConfig,
    query: &str,
    passages: &[&str],
    limit: usize,
) -> Result<Vec<Scored>, RagError> {
    if config.url.trim().is_empty() {
        return Err(RagError::Unavailable(
            "backend rerank HTTP chưa có URL".into(),
        ));
    }
    let base = config.url.trim().trim_end_matches('/');
    let endpoint = if base.ends_with("/rerank") {
        base.to_owned()
    } else {
        format!("{base}/v1/rerank")
    };
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| RagError::Service(format!("không dựng được HTTP reranker: {error}")))?;
    let mut request = client.post(&endpoint).json(&Request {
        model: (!config.model.trim().is_empty()).then_some(config.model.as_str()),
        query,
        documents: passages,
    });
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }
    let response = request.send().await.map_err(|error| {
        RagError::Service(format!(
            "không gọi được máy chủ rerank ở {endpoint}: {error}"
        ))
    })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(RagError::Service(format!(
            "máy chủ rerank trả {status}: {detail}"
        )));
    }
    let result = response
        .json::<Response>()
        .await
        .map_err(|error| RagError::Service(format!("phản hồi rerank không hợp lệ: {error}")))?
        .results;
    let mut scores = vec![f32::NEG_INFINITY; passages.len()];
    for item in result {
        if let Some(slot) = scores.get_mut(item.index) {
            *slot = item.score;
        }
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .enumerate()
        .map(|(index, score)| Scored { index, score })
        .collect();
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));
    ranked.truncate(limit.min(passages.len()));
    Ok(ranked)
}
