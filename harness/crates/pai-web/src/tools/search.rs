//! `web.search` — ask a search engine, get a list back.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::search::{SearchHit, SearchProvider};

/// Default result count. Eight titles with excerpts is roughly 600 tokens: enough to choose what
/// to `web.fetch` next, which is the only job this tool has.
const DEFAULT_LIMIT: usize = 8;
/// Hard cap; also the most any provider here accepts.
const MAX_LIMIT: usize = 20;
/// A search that has not answered in half a minute has failed, whatever it says later.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Excerpts arrive at wildly different lengths; a runaway one would crowd out the other results.
const MAX_SNIPPET_CHARS: usize = 400;
/// Titles come from the same stranger the excerpts do, and nothing about the protocol bounds them.
const MAX_TITLE_CHARS: usize = 200;
/// Ceiling on the whole rendered list, and the only one that holds unconditionally: the two above
/// bound the fields this tool knows how to shorten, while a URL cannot be shortened without being
/// broken. Twenty results of title, URL and excerpt fit inside this with room to spare, so it bites
/// only on a provider that answers with something other than a page of results.
const MAX_OUTPUT_CHARS: usize = 16_000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchArgs {
    /// Câu hoặc cụm từ cần tìm, viết như khi gõ vào ô tìm kiếm.
    pub query: String,
    /// Số kết quả tối đa. Mặc định 8, trần 20.
    pub limit: Option<usize>,
}

pub struct WebSearch {
    provider: Arc<dyn SearchProvider>,
}

impl WebSearch {
    pub const NAME: &'static str = "web.search";

    pub fn new(provider: Arc<dyn SearchProvider>) -> WebSearch {
        WebSearch { provider }
    }
}

#[async_trait]
impl Tool for WebSearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            WebSearch::NAME,
            "Tìm trên web và trả về danh sách kết quả gồm tiêu đề, địa chỉ và đoạn trích ngắn. \
             Đoạn trích chỉ đủ để chọn nên đọc trang nào; muốn biết nội dung thật thì gọi tiếp \
             `web.fetch` với địa chỉ trong kết quả.",
            json_schema_for::<WebSearchArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // `leaving_device`: the query itself goes to a third party, which is often the more
        // sensitive half of a search. `untrusted`: titles and excerpts are attacker-writable text.
        ToolMeta::read_only()
            .leaving_device()
            .untrusted()
            .concurrency_safe(true)
            .with_timeout(TIMEOUT)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: WebSearchArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let query = args.query.trim();
        if query.is_empty() {
            return Err(ToolError::Invalid("`query` không được rỗng.".to_string()));
        }
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

        let cancel = call.cancel_token();
        let hits = self
            .provider
            .search(query, limit, &cancel)
            .await
            // Every failure here is `Failed`, including a missing key: `Invalid` tells the model to
            // rewrite its arguments, and no rewording of a query conjures an API key. The text
            // carries the fix, and the person reading it is the one who can apply it.
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if hits.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "`{query}` không có kết quả nào trên {}.",
                self.provider.name()
            )));
        }

        // Trust the cap here as well as at the provider: a provider that ignores `count` must not
        // be able to decide how much of the context window a search costs.
        let hits: Vec<SearchHit> = hits.into_iter().take(limit).collect();
        let rendered = hits
            .iter()
            .enumerate()
            .map(|(index, hit)| render(index + 1, hit))
            .collect::<Vec<_>>()
            .join("\n\n");
        // Belt over the per-field braces: the count is capped and the fields are clipped, but a URL
        // has no length this tool may impose, so the assembled list gets the last ceiling.
        let rendered = pai_web_core::trim_to(&rendered, MAX_OUTPUT_CHARS).text;

        let structured = json!({
            "provider": self.provider.name(),
            "query": query,
            "results": hits,
        });
        Ok(ToolOutcome::ok(rendered).with_structured(structured))
    }
}

/// One result, shaped so a model can quote the URL back into `web.fetch` without editing it.
///
/// Both fields are cut with [`pai_web_core::clip`] rather than `trim_to`: a three-line notice
/// hanging off one item of a twenty-item list explains less than the ellipsis it replaces, and
/// the list as a whole is still bounded by [`MAX_OUTPUT_CHARS`], which does say when it cuts.
fn render(rank: usize, hit: &SearchHit) -> String {
    let snippet = pai_web_core::clip(hit.snippet.trim(), MAX_SNIPPET_CHARS);
    let title = match hit.title.trim() {
        "" => "(không có tiêu đề)".to_string(),
        title => pai_web_core::clip(title, MAX_TITLE_CHARS),
    };
    format!("{rank}. {title}\n   {}\n   {snippet}", hit.url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pai_tools::{Tool, ToolName};
    use serde_json::{Map, Value};

    use crate::fake::FakeSearch;
    use crate::search::SearchError;
    use crate::search::brave::KEY_ENV;

    fn hit(title: &str, url: &str, snippet: &str) -> SearchHit {
        SearchHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
        }
    }

    fn goi(args: Value) -> Invocation {
        let arguments = match args {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Invocation::new(ToolName::new(WebSearch::NAME), "call-1", arguments)
    }

    fn tool(provider: FakeSearch) -> WebSearch {
        WebSearch::new(Arc::new(provider))
    }

    #[test]
    fn khai_bao_dung_chinh_sach() {
        let meta = tool(FakeSearch::hits(Vec::new())).meta();
        assert!(!meta.mutating);
        assert!(meta.leaves_device, "câu truy vấn rời khỏi máy, phải khai");
        assert!(meta.returns_untrusted_content);
        assert!(meta.timeout < pai_tools::schema::DEFAULT_TIMEOUT);
    }

    #[tokio::test]
    async fn liet_ke_ket_qua_kem_url_de_fetch_tiep() {
        let provider = FakeSearch::hits(vec![
            hit("Rust async", "https://vidu.test/a", "Giới thiệu async."),
            hit("Tokio", "https://vidu.test/b", "Runtime bất đồng bộ."),
        ]);
        let outcome = tool(provider)
            .execute(&goi(serde_json::json!({ "query": "rust async" })))
            .await
            .expect("chạy được");

        let text = &outcome.content;
        assert!(text.contains("1. Rust async"), "{text}");
        assert!(text.contains("https://vidu.test/a"), "{text}");
        assert!(text.contains("2. Tokio"), "{text}");
        assert!(text.contains("Runtime bất đồng bộ."), "{text}");

        let structured = outcome.structured.expect("structured cho UI");
        assert_eq!(structured["results"].as_array().map(Vec::len), Some(2));
        assert_eq!(structured["query"], serde_json::json!("rust async"));
    }

    #[tokio::test]
    async fn ton_trong_tran_so_ket_qua_ke_ca_khi_nha_cung_cap_tra_thua() {
        let provider = FakeSearch::hits(
            (0..50)
                .map(|i| hit(&format!("Kết quả {i}"), &format!("https://vidu.test/{i}"), "x"))
                .collect(),
        );
        let outcome = tool(provider)
            .execute(&goi(serde_json::json!({ "query": "x", "limit": 999 })))
            .await
            .expect("chạy được");
        let count = outcome.structured.expect("structured")["results"]
            .as_array()
            .map(Vec::len)
            .expect("mảng");
        assert_eq!(count, MAX_LIMIT, "phải bị kẹp về trần cứng");
    }

    #[tokio::test]
    async fn doan_trich_va_tieu_de_dai_deu_bi_cat_va_danh_dau() {
        let provider = FakeSearch::hits(vec![hit(
            &"Tiêu đề dài ".repeat(200),
            "https://vidu.test/dai",
            &"chữ ".repeat(2000),
        )]);
        let outcome = tool(provider)
            .execute(&goi(serde_json::json!({ "query": "x" })))
            .await
            .expect("chạy được");
        let text = &outcome.content;
        assert!(text.chars().count() < 1000, "{text}");
        assert_eq!(text.matches('…').count(), 2, "cả tiêu đề lẫn đoạn trích phải báo là bị cắt: {text}");
        assert!(text.contains("https://vidu.test/dai"), "url phải nguyên vẹn để fetch tiếp: {text}");
    }

    /// The ceiling that holds when the per-field ones cannot: a URL has no length this tool may
    /// impose, so a provider answering with enormous URLs must still not decide the token bill.
    #[tokio::test]
    async fn ca_danh_sach_van_co_tran_khi_url_dai_bat_thuong() {
        let provider = FakeSearch::hits(
            (0..MAX_LIMIT)
                .map(|i| {
                    hit(
                        "Kết quả",
                        &format!("https://vidu.test/{}", "d".repeat(20_000 + i)),
                        "ngắn",
                    )
                })
                .collect(),
        );
        let outcome = tool(provider)
            .execute(&goi(serde_json::json!({ "query": "x", "limit": 20 })))
            .await
            .expect("chạy được");
        assert!(
            outcome.content.chars().count() <= MAX_OUTPUT_CHARS + 200,
            "vượt trần: {} ký tự",
            outcome.content.chars().count()
        );
        assert!(outcome.content.contains("Đã cắt bớt"), "phải nói rõ đã cắt");
    }

    #[tokio::test]
    async fn khong_co_ket_qua_thi_noi_ro_la_khong_co() {
        let outcome = tool(FakeSearch::hits(Vec::new()))
            .execute(&goi(serde_json::json!({ "query": "chuoi khong ai viet" })))
            .await
            .expect("chạy được");
        assert!(!outcome.is_error);
        assert!(outcome.content.contains("không có kết quả"), "{}", outcome.content);
    }

    /// A missing key must reach the user as a sentence naming the variable, not as an empty list
    /// that reads like "the web has nothing on this".
    #[tokio::test]
    async fn thieu_khoa_thi_bao_thieu_khoa_chu_khong_tra_rong() {
        let provider = FakeSearch::failing(|| SearchError::MissingKey {
            provider: "Brave Search",
            env: KEY_ENV,
        });
        let err = tool(provider)
            .execute(&goi(serde_json::json!({ "query": "x" })))
            .await
            .expect_err("phải là lỗi");
        assert!(err.to_string().contains(KEY_ENV), "{err}");
    }

    #[tokio::test]
    async fn query_rong_bi_tu_choi_som() {
        let err = tool(FakeSearch::hits(Vec::new()))
            .execute(&goi(serde_json::json!({ "query": "   " })))
            .await
            .expect_err("query rỗng");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
    }
}
