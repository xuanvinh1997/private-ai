//! Brave Search, the default provider.
//!
//! Chosen because it is the one this product used to reach through an `npx` MCP server, so
//! replacing that server changes nothing a user can see. The key never appears in a log line, a
//! `Debug` render or an error message: [`Brave`] implements `Debug` by hand for exactly that
//! reason, since a `#[derive]` here would print the credential into any `tracing` span that
//! touched it.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Response};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{SearchError, SearchHit, SearchProvider};

const PROVIDER: &str = "Brave Search";
/// Where the key is read from when the application does not pass one in.
pub const KEY_ENV: &str = "BRAVE_SEARCH_API_KEY";
const ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
/// Brave's own name for the header. It is a bearer credential in all but name.
const KEY_HEADER: &str = "X-Subscription-Token";
/// Brave rejects `count` above 20, so clamping here turns a would-be HTTP 422 into fewer results.
const MAX_COUNT: usize = 20;
/// What the client this provider is handed should use. A fixed, trusted endpoint is still an
/// endpoint that can stop answering mid-handshake, and the tool deadline above it is a backstop,
/// not a plan.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Ceiling on the response body. Twenty results with excerpts are tens of kilobytes; two megabytes
/// is room for a provider having a strange day and no room for one having an infinite one. Read
/// with a ceiling for the reason [`crate::fetch`] streams: `text()` allocates whatever the far end
/// decides to send, and `Content-Length` is a claim.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Brave's response, narrowed to the three fields worth reading. `serde` ignores the rest, which
/// is what keeps this from breaking every time the provider adds a field.
#[derive(Debug, Deserialize)]
struct BraveResponse {
    web: Option<BraveWeb>,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Debug, Deserialize)]
struct BraveResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    /// Comes back with `<strong>` around the matched words.
    #[serde(default)]
    description: String,
}

pub struct Brave {
    client: Client,
    /// `None` means the application had no key to give; every call then fails loudly.
    key: Option<String>,
}

/// Hand-written so the key cannot leak through a formatter.
impl fmt::Debug for Brave {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Brave")
            .field("key", &if self.key.is_some() { "<đã đặt>" } else { "<thiếu>" })
            .finish()
    }
}

impl Brave {
    /// Takes the key rather than reading the environment itself: configuration is the plugin's
    /// job, and a provider that reads its own environment cannot be tested without setting one.
    pub fn new(client: Client, key: Option<String>) -> Brave {
        Brave {
            client,
            // An empty or whitespace-only variable is a key that was never set, not a key that is "".
            key: key.filter(|value| !value.trim().is_empty()),
        }
    }
}

#[async_trait]
impl SearchProvider for Brave {
    fn name(&self) -> &'static str {
        PROVIDER
    }

    async fn search(
        &self,
        query: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let Some(key) = self.key.as_deref() else {
            return Err(SearchError::MissingKey {
                provider: PROVIDER,
                env: KEY_ENV,
            });
        };

        let request = self
            .client
            .get(ENDPOINT)
            .query(&[
                ("q", query),
                ("count", &limit.clamp(1, MAX_COUNT).to_string()),
            ])
            .header(KEY_HEADER, key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send();

        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(SearchError::Cancelled),
            result = request => result.map_err(|err| SearchError::Transport(err.to_string()))?,
        };

        let status = response.status();
        if !status.is_success() {
            // The body may repeat the key back in an error envelope, so only the status travels.
            return Err(SearchError::Status {
                provider: PROVIDER,
                status: status.as_u16(),
            });
        }

        let body = read_body(response, cancel).await?;
        parse(&body)
    }
}

/// Read the response with a ceiling, and stop pulling the moment the caller gives up.
async fn read_body(
    mut response: Response,
    cancel: &CancellationToken,
) -> Result<String, SearchError> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(SearchError::Cancelled),
            chunk = response.chunk() => chunk.map_err(|err| SearchError::Transport(err.to_string()))?,
        };
        let Some(chunk) = chunk else { break };
        if bytes.len() + chunk.len() > MAX_BODY_BYTES {
            // No salvage attempt: half a JSON document parses into nothing, so a truncated read is
            // reported as a broken answer rather than quietly becoming an empty result list.
            return Err(SearchError::Malformed(format!(
                "phản hồi dài quá {MAX_BODY_BYTES} byte"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|err| SearchError::Malformed(err.to_string()))
}

/// Split out from the call so the response shape is testable without a socket or a key.
fn parse(body: &str) -> Result<Vec<SearchHit>, SearchError> {
    let parsed: BraveResponse =
        serde_json::from_str(body).map_err(|err| SearchError::Malformed(err.to_string()))?;
    Ok(parsed
        .web
        .map(|web| web.results)
        .unwrap_or_default()
        .into_iter()
        // A result with no URL is nothing the model can follow up on.
        .filter(|result| !result.url.trim().is_empty())
        .map(|result| SearchHit {
            title: pai_web_core::strip_tags(&result.title),
            url: result.url,
            snippet: pai_web_core::strip_tags(&result.description),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn thieu_khoa_thi_bao_ro_thieu_gi() {
        let brave = Brave::new(Client::new(), None);
        let err = brave
            .search("gì cũng được", 5, &CancellationToken::new())
            .await
            .expect_err("không có khoá thì phải lỗi");
        assert!(matches!(err, SearchError::MissingKey { .. }), "{err}");
        let message = err.to_string();
        assert!(message.contains(KEY_ENV), "{message}");
        assert!(message.contains("chưa có khoá API"), "{message}");
    }

    #[tokio::test]
    async fn khoa_rong_bi_coi_la_khong_co_khoa() {
        let brave = Brave::new(Client::new(), Some("   ".to_string()));
        assert!(matches!(
            brave
                .search("x", 5, &CancellationToken::new())
                .await
                .expect_err("khoá rỗng vẫn là thiếu khoá"),
            SearchError::MissingKey { .. }
        ));
    }

    #[test]
    fn debug_khong_lo_khoa() {
        let brave = Brave::new(Client::new(), Some("bi-mat-tuyet-doi".to_string()));
        let rendered = format!("{brave:?}");
        assert!(!rendered.contains("bi-mat-tuyet-doi"), "{rendered}");
        assert!(rendered.contains("<đã đặt>"), "{rendered}");
    }

    #[test]
    fn doc_duoc_ket_qua_that() {
        let body = r#"{
          "web": { "results": [
            { "title": "Rust <strong>async</strong>", "url": "https://vidu.test/a", "description": "Giới thiệu <strong>async</strong> trong Rust." },
            { "title": "Không có url", "url": "  ", "description": "bỏ qua" }
          ]}
        }"#;
        let hits = parse(body).expect("parse");
        assert_eq!(hits.len(), 1, "kết quả không có url phải bị loại");
        assert_eq!(hits[0].title, "Rust async");
        assert_eq!(hits[0].url, "https://vidu.test/a");
        assert_eq!(hits[0].snippet, "Giới thiệu async trong Rust.");
    }

    #[test]
    fn khong_co_muc_web_thi_la_rong_chu_khong_phai_loi() {
        assert_eq!(parse(r#"{"query":{"original":"x"}}"#).expect("parse").len(), 0);
    }

    #[test]
    fn json_hong_thi_bao_hong() {
        assert!(matches!(
            parse("khong phai json").expect_err("phải lỗi"),
            SearchError::Malformed(_)
        ));
    }
}
