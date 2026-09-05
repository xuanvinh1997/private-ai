//! `web.fetch` — read one URL as Markdown.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::fetch::Fetcher;

/// Caller default. Roughly 10k tokens: a whole long article, and still a minority of any
/// reasonable context window.
const DEFAULT_MAX_CHARS: usize = 40_000;
/// Hard ceiling, above which the parameter is ignored. A model asking for a million characters is
/// asking for the conversation to be evicted, whatever it thinks it is asking for.
const MAX_MAX_CHARS: usize = 120_000;
/// The tool's own deadline, above [`crate::fetch::DEFAULT_TIMEOUT`] because one fetch may be
/// several hops, and below the pipeline's 120s because nothing here is worth two minutes.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
/// Ceiling on the page's own title. `<title>` is text a stranger wrote and it is printed *outside*
/// the trimmed body, so without its own ceiling it is a way around `max_chars` altogether: a page
/// with a megabyte-long title would spend the whole budget before the article started.
const MAX_TITLE_CHARS: usize = 200;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebFetchArgs {
    /// Địa chỉ trang cần đọc, phải bắt đầu bằng `http://` hoặc `https://`.
    pub url: String,
    /// Số ký tự tối đa muốn nhận. Mặc định 40000, trần 120000. Nội dung dài hơn bị cắt và
    /// phần trả về sẽ nói rõ đã cắt.
    pub max_chars: Option<usize>,
}

pub struct WebFetch {
    fetcher: Arc<Fetcher>,
}

impl WebFetch {
    pub const NAME: &'static str = "web.fetch";

    pub fn new(fetcher: Arc<Fetcher>) -> WebFetch {
        WebFetch { fetcher }
    }
}

#[async_trait]
impl Tool for WebFetch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            WebFetch::NAME,
            "Tải một trang web rồi trả về nội dung chính dưới dạng Markdown: đã bỏ menu, \
             quảng cáo, chân trang và mã script, còn giữ tiêu đề, danh sách, bảng, khối mã và \
             liên kết. Đọc được cả HTML, JSON, Markdown và văn bản thuần; ảnh, video hay tệp \
             nhị phân thì báo lỗi chứ không đọc. Chỉ nhận `http`/`https` ra Internet công cộng: \
             địa chỉ nội bộ và localhost bị từ chối.",
            json_schema_for::<WebFetchArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // `leaving_device`: the URL and this machine's IP reach a third party. `untrusted`: the
        // body is whatever a stranger chose to serve, and a page that asks the model to do
        // something is a page, not an instruction.
        ToolMeta::read_only()
            .leaving_device()
            .untrusted()
            .concurrency_safe(true)
            .with_timeout(TIMEOUT)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: WebFetchArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let budget = args
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .clamp(1, MAX_MAX_CHARS);

        let cancel = call.cancel_token();
        let fetched = self
            .fetcher
            .fetch(args.url.trim(), &cancel)
            .await
            // A blocked URL is the model's mistake to fix, not a failure of the machine, so it
            // comes back as `Invalid` and the model can try a different address.
            .map_err(|err| match err {
                crate::fetch::FetchError::Blocked(_) | crate::fetch::FetchError::BadUrl(_) => {
                    ToolError::Invalid(err.to_string())
                }
                other => ToolError::Failed(other.to_string()),
            })?;

        let page = pai_web_core::render(
            &fetched.bytes,
            fetched.content_type.as_deref(),
            Some(&fetched.url),
        )
        .map_err(|err| ToolError::Failed(err.to_string()))?;

        let trimmed = pai_web_core::trim_to(&page.markdown, budget);

        let mut text = String::new();
        if let Some(title) = &page.title {
            text.push_str(&format!(
                "# {}\n",
                pai_web_core::clip(title, MAX_TITLE_CHARS)
            ));
        }
        text.push_str(&format!("Nguồn: {}\n\n", fetched.url));
        if fetched.truncated {
            // Two different ceilings can cut a page; saying which one did it is the difference
            // between "ask for more characters" and "this page is simply enormous".
            text.push_str(
                "(Tải bị dừng ở trần dung lượng nên trang này chưa về hết.)\n\n",
            );
        }
        text.push_str(if trimmed.text.trim().is_empty() {
            "(Trang không có nội dung văn bản nào đọc được.)"
        } else {
            &trimmed.text
        });

        let structured = json!({
            "url": fetched.url,
            "status": fetched.status,
            "title": page.title,
            "media": page.media,
            "encoding": page.encoding,
            "chars": trimmed.kept,
            "total_chars": trimmed.total,
            "truncated": trimmed.truncated || fetched.truncated,
        });
        Ok(ToolOutcome::ok(text).with_structured(structured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pai_tools::{Tool, ToolName};
    use serde_json::{Map, Value};

    use crate::fake::{Reply, routes, serve};
    use crate::fetch::Limits;
    use crate::guard::Guard;

    const TRANG: &str = r#"<!doctype html><html><head><title>Bài mẫu</title>
        <script>theo_doi()</script></head><body>
        <nav><a href="/menu">Thực đơn</a></nav>
        <main><h1>Bài mẫu</h1><p>Một đoạn văn thật.</p>
        <p>Xem <a href="/tiep">phần tiếp</a>.</p></main>
        <footer>Bản quyền</footer></body></html>"#;

    fn tool(limits: Limits) -> WebFetch {
        WebFetch::new(Arc::new(
            crate::fetch::Fetcher::new(Guard::lenient(), limits).expect("dựng Fetcher"),
        ))
    }

    fn goi(args: Value) -> Invocation {
        let arguments = match args {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Invocation::new(ToolName::new(WebFetch::NAME), "call-1", arguments)
    }

    #[test]
    fn khai_bao_dung_chinh_sach() {
        let meta = tool(Limits::default()).meta();
        assert!(!meta.mutating, "chỉ đọc");
        assert!(meta.leaves_device, "tool này ra khỏi máy, phải khai");
        assert!(meta.returns_untrusted_content, "nội dung web là dữ liệu lạ");
        assert!(
            meta.timeout < pai_tools::schema::DEFAULT_TIMEOUT,
            "phải ngắn hơn mặc định 120s"
        );
    }

    #[test]
    fn mo_ta_bi_noi_them_canh_bao_khong_tin_cay() {
        let tool = tool(Limits::default());
        let framed = tool.meta().frame(&tool.schema().description);
        assert!(framed.contains(pai_tools::UNTRUSTED_NOTICE), "{framed}");
    }

    #[tokio::test]
    async fn tra_ve_markdown_sach_kem_nguon() {
        let addr = serve(routes([("/bai", Reply::html(TRANG))])).await;
        let outcome = tool(Limits::default())
            .execute(&goi(serde_json::json!({ "url": format!("http://{addr}/bai") })))
            .await
            .expect("chạy được");

        let text = &outcome.content;
        assert!(text.contains("# Bài mẫu"), "{text}");
        assert!(text.contains(&format!("Nguồn: http://{addr}/bai")), "{text}");
        assert!(text.contains("Một đoạn văn thật"), "{text}");
        assert!(!text.contains("theo_doi"), "script phải biến mất: {text}");
        assert!(!text.contains("Thực đơn"), "nav phải biến mất: {text}");
        assert!(!text.contains("Bản quyền"), "footer phải biến mất: {text}");
        // Relative links only become useful once resolved against the page they came from.
        assert!(text.contains(&format!("http://{addr}/tiep")), "{text}");

        let structured = outcome.structured.expect("có structured cho UI");
        assert_eq!(structured["media"], serde_json::json!("html"));
        assert_eq!(structured["status"], serde_json::json!(200));
        assert_eq!(structured["truncated"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn ton_trong_max_chars_va_noi_ro_da_cat() {
        let addr = serve(routes([("/bai", Reply::html(TRANG))])).await;
        let outcome = tool(Limits::default())
            .execute(&goi(
                serde_json::json!({ "url": format!("http://{addr}/bai"), "max_chars": 20 }),
            ))
            .await
            .expect("chạy được");
        assert!(outcome.content.contains("Đã cắt bớt"), "{}", outcome.content);
        assert_eq!(
            outcome.structured.expect("structured")["truncated"],
            serde_json::json!(true)
        );
    }

    /// The title is printed outside the trimmed body, so it needs a ceiling of its own or it is a
    /// way around `max_chars`.
    #[tokio::test]
    async fn tieu_de_dai_bat_thuong_van_bi_kep() {
        let trang = format!(
            "<html><head><title>{}</title></head><body><main><p>Thân bài.</p></main></body></html>",
            "dài ".repeat(5_000)
        );
        let addr = serve(routes([("/dai", Reply::html(&trang))])).await;
        let outcome = tool(Limits::default())
            .execute(&goi(serde_json::json!({ "url": format!("http://{addr}/dai") })))
            .await
            .expect("chạy được");
        let heading = outcome
            .content
            .lines()
            .next()
            .expect("dòng tiêu đề")
            .to_string();
        assert!(heading.chars().count() <= MAX_TITLE_CHARS + 2, "{heading}");
        assert!(heading.ends_with('…'), "phải nói rõ tiêu đề bị cắt: {heading}");
        assert!(outcome.content.contains("Thân bài"), "{}", outcome.content);
    }

    #[tokio::test]
    async fn dia_chi_noi_bo_la_loi_tham_so_de_mo_hinh_sua_duoc() {
        let err = tool(Limits::default())
            .execute(&goi(serde_json::json!({ "url": "http://169.254.169.254/" })))
            .await
            .expect_err("phải bị chặn");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("mạng nội bộ"), "{err}");
    }

    #[tokio::test]
    async fn anh_bi_tu_choi_thay_vi_do_byte_vao_ngu_canh() {
        let addr = serve(routes([(
            "/anh.png",
            Reply {
                status: 200,
                headers: vec![("Content-Type".into(), "image/png".into())],
                body: vec![0x89, b'P', b'N', b'G', 0, 0, 0, 13],
            },
        )]))
        .await;
        let err = tool(Limits::default())
            .execute(&goi(serde_json::json!({ "url": format!("http://{addr}/anh.png") })))
            .await
            .expect_err("ảnh không đọc được");
        assert!(err.to_string().contains("không phải văn bản"), "{err}");
    }

    #[tokio::test]
    async fn thieu_url_la_tham_so_khong_hop_le() {
        let err = tool(Limits::default())
            .execute(&goi(serde_json::json!({})))
            .await
            .expect_err("thiếu `url`");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
    }
}
