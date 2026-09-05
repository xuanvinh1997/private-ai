//! Native core for the web layer.
//!
//! This crate intentionally has no network, model or tool dependencies. It owns the
//! deterministic parts: turning a fetched byte stream into Markdown a model can read,
//! so every rule here is testable without a socket.
//!
//! The one function worth reading first is [`render`]. Everything else -- charset ladder, media
//! classification, boilerplate rules, the size ceiling -- is a step inside it, split into its own
//! module so each step can be argued with in isolation.

pub mod decode;
pub mod html;
pub mod media;
pub mod trim;

pub use decode::{Decoded, decode};
pub use html::{Article, strip_tags, to_markdown};
pub use media::{ContentType, Media, classify};
pub use trim::{Trimmed, clip, trim_to};

use serde::Serialize;

/// How many bytes of the body are enough to tell text from a JPEG. The magic numbers and the
/// `<html` of every real document live well inside the first kilobyte.
const SNIFF_WINDOW: usize = 1024;

/// The `Content-Type` values that mean "the server does not know", and only those.
///
/// The list is short on purpose. Sniffing past a server that *did* name a type is how a PDF ends
/// up in a model's context: `%PDF-1.7` has no NUL in its first kilobyte, so the sniffer below
/// would happily call it text and hand over the raw file. A named type is believed even when the
/// answer is "not readable"; only a shrug is overruled.
const UNCERTAIN_TYPES: &[&str] = &["application/octet-stream", "binary/octet-stream", ""];

/// One fetched resource, rendered for a model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page {
    /// The page's own title, when it had one.
    pub title: Option<String>,
    pub markdown: String,
    pub media: Media,
    /// Which encoding the bytes turned out to be in; useful when a page comes back as mojibake.
    pub encoding: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// Not text and not going to become text; the caller should say so rather than show the model bytes.
    #[error("nội dung kiểu `{0}` không phải văn bản nên không đọc được")]
    NotText(String),
    #[error("máy chủ trả về nội dung rỗng")]
    Empty,
}

/// Render fetched bytes into Markdown.
///
/// `content_type` is the server's `Content-Type` header and `base_url` the address the bytes came
/// from, used to resolve relative links. The header is trusted for the charset but only
/// half-trusted for the kind: `application/octet-stream` and a missing header both mean "the server
/// does not know", and in that case the bytes themselves get the last word.
pub fn render(
    bytes: &[u8],
    content_type: Option<&str>,
    base_url: Option<&str>,
) -> Result<Page, RenderError> {
    if bytes.is_empty() {
        return Err(RenderError::Empty);
    }

    let declared = content_type.map(ContentType::parse);
    let media = match declared.as_ref() {
        // A server that says nothing, or says "some bytes", has told us nothing worth believing.
        None => sniff(bytes),
        Some(declared) if UNCERTAIN_TYPES.contains(&declared.essence.as_str()) => sniff(bytes),
        Some(declared) => declared.media(),
    };
    if let Media::Other(essence) = &media {
        return Err(RenderError::NotText(essence.clone()));
    }

    let decoded = decode(bytes, declared.as_ref().and_then(|ct| ct.charset.as_deref()));

    let (title, markdown) = match media {
        Media::Html => {
            let article = to_markdown(&decoded.text, base_url);
            (article.title, article.markdown)
        }
        // Fenced rather than reformatted: pretty-printing would need a JSON parser here, and a model
        // reads minified JSON fine as long as it is told that is what it is looking at.
        Media::Json => (None, fenced(decoded.text.trim(), "json")),
        // Markdown and plain text are already what the model wants; passing them through an HTML
        // parser would mangle every `<` they contain.
        Media::Markdown | Media::Text => (None, decoded.text.trim().to_string()),
        Media::Other(essence) => return Err(RenderError::NotText(essence)),
    };

    Ok(Page {
        title,
        markdown,
        media,
        encoding: decoded.encoding,
    })
}

/// Wrap a body in a code fence long enough that the body cannot close it.
///
/// A fixed three-backtick fence is escapable: a JSON string containing ``` ends the block early,
/// and everything after it stops reading as quoted data and starts reading as the page's own
/// structure. Since the body here is written by a stranger, that is the difference between quoting
/// a document and letting it talk.
fn fenced(body: &str, lang: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for byte in body.bytes() {
        run = if byte == b'`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{lang}\n{body}\n{fence}")
}

/// Guess the kind from the bytes, for when the server would not say.
fn sniff(bytes: &[u8]) -> Media {
    let head = &bytes[..bytes.len().min(SNIFF_WINDOW)];
    // A NUL byte in the first kilobyte is the oldest and still the best binary test: no text
    // encoding this crate can decode puts one there.
    if head.contains(&0) {
        return Media::Other("application/octet-stream".to_string());
    }
    let text = String::from_utf8_lossy(head);
    let start = text.trim_start().to_ascii_lowercase();
    if start.starts_with("<!doctype html") || start.starts_with("<html") || start.contains("<html")
    {
        Media::Html
    } else if start.starts_with('{') || start.starts_with('[') {
        Media::Json
    } else {
        Media::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_ra_markdown() {
        let page = render(
            b"<html><head><title>Chao</title></head><body><main><h1>Chao</h1><p>Xin chao.</p></main></body></html>",
            Some("text/html; charset=utf-8"),
            Some("https://vidu.test/"),
        )
        .expect("render");
        assert_eq!(page.media, Media::Html);
        assert_eq!(page.title.as_deref(), Some("Chao"));
        assert!(page.markdown.contains("# Chao"), "{}", page.markdown);
    }

    #[test]
    fn json_khong_di_qua_duong_html() {
        let page = render(br#"{"a":"<b>x</b>"}"#, Some("application/json"), None).expect("render");
        assert_eq!(page.media, Media::Json);
        assert!(page.markdown.starts_with("```json"));
        assert!(page.markdown.contains("<b>x</b>"), "{}", page.markdown);
    }

    #[test]
    fn text_thuan_giu_nguyen_dau_nhon() {
        let page = render(b"a < b && c > d", Some("text/plain"), None).expect("render");
        assert_eq!(page.media, Media::Text);
        assert_eq!(page.markdown, "a < b && c > d");
    }

    #[test]
    fn markdown_giu_nguyen() {
        let page = render(b"# Tieu de\n\n- mot\n- hai", Some("text/markdown"), None).expect("render");
        assert_eq!(page.media, Media::Markdown);
        assert!(page.markdown.starts_with("# Tieu de"));
    }

    #[test]
    fn octet_stream_van_duoc_ngui_lai() {
        let page = render(
            b"<html><body><main><p>Van la html.</p></main></body></html>",
            Some("application/octet-stream"),
            None,
        )
        .expect("render");
        assert_eq!(page.media, Media::Html);
    }

    /// The other half of that rule: a server that *did* name a binary type is believed, so a PDF
    /// whose first kilobyte happens to be NUL-free never gets sniffed into `Media::Text`.
    #[test]
    fn kieu_nhi_phan_da_khai_thi_khong_ngui_lai() {
        let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n%%EOF\n";
        let err = render(pdf, Some("application/pdf"), None).expect_err("PDF không phải văn bản");
        assert!(
            err.to_string().contains("application/pdf"),
            "lỗi phải gọi đúng tên kiểu máy chủ khai: {err}"
        );
    }

    #[test]
    fn json_khong_pha_duoc_hang_rao_code_fence() {
        let page = render(br#"{"a":"``` xin chao"}"#, Some("application/json"), None).expect("render");
        let fence = page
            .markdown
            .lines()
            .next()
            .expect("dòng mở hàng rào")
            .trim_end_matches("json");
        assert!(fence.len() > 3, "hàng rào phải dài hơn dãy backtick bên trong: {}", page.markdown);
        assert!(page.markdown.ends_with(fence), "{}", page.markdown);
    }

    #[test]
    fn anh_bi_tu_choi_chu_khong_hien_byte() {
        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        let err = render(&png, None, None).expect_err("ảnh phải bị từ chối");
        assert!(matches!(err, RenderError::NotText(_)), "{err}");
        assert!(err.to_string().contains("không phải văn bản"));
    }

    #[test]
    fn rong_la_loi_rieng() {
        assert!(matches!(render(b"", Some("text/html"), None), Err(RenderError::Empty)));
    }

    #[test]
    fn page_serialize_duoc_cho_ui() {
        let page = render(b"xin chao", Some("text/plain"), None).expect("render");
        let json = serde_json::to_value(&page).expect("serialize");
        assert_eq!(json["media"], serde_json::json!("text"));
    }
}
