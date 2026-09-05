//! What kind of bytes arrived.
//!
//! One `Content-Type` header answers two separate questions -- which charset the bytes are in,
//! and which renderer they belong to -- so it is parsed once, here, and never re-sniffed
//! downstream. Servers get this header wrong often enough that [`crate::render`] is allowed to
//! overrule it, but only from a single place.

use serde::Serialize;

/// The renderers this crate has. Everything else is refused rather than guessed at, because a
/// model reading mojibake from a JPEG is worse than a model told the page was not text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Html,
    Json,
    Markdown,
    /// Plain text of any flavour: `text/plain`, XML, YAML, source code.
    Text,
    /// Not text; carries the essence so the refusal can name what it refused.
    Other(String),
}

impl Media {
    /// Whether this crate can turn it into something a model reads.
    pub fn readable(&self) -> bool {
        !matches!(self, Media::Other(_))
    }
}

/// A parsed `Content-Type`, split into the two things anyone downstream actually wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentType {
    /// Lowercased type/subtype with parameters removed.
    pub essence: String,
    /// The `charset` parameter, lowercased, if the server sent one.
    pub charset: Option<String>,
}

impl ContentType {
    pub fn parse(raw: &str) -> ContentType {
        let mut parts = raw.split(';');
        let essence = parts
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let charset = parts.find_map(|param| {
            let (name, value) = param.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("charset") {
                return None;
            }
            // Quoting is legal here and `encoding_rs` will not recognise `"utf-8"` with the quotes.
            let value = value.trim().trim_matches(['"', '\'']).trim();
            (!value.is_empty()).then(|| value.to_ascii_lowercase())
        });
        ContentType { essence, charset }
    }

    pub fn media(&self) -> Media {
        classify(&self.essence)
    }
}

/// Map an essence to a renderer. Suffix rules (`+json`, `+xml`) come from RFC 6839 and catch the
/// long tail -- `application/ld+json`, `application/atom+xml` -- that an exact list never does.
pub fn classify(essence: &str) -> Media {
    match essence {
        "text/html" | "application/xhtml+xml" => Media::Html,
        "application/json" | "text/json" => Media::Json,
        "text/markdown" | "text/x-markdown" => Media::Markdown,
        // `application/octet-stream` is what a server says when it does not know; sniffing the
        // bytes beats believing it, so it is reported as unreadable and [`crate::render`] retries.
        _ if essence.ends_with("+json") => Media::Json,
        _ if essence.ends_with("+xml") => Media::Text,
        _ if essence.starts_with("text/") => Media::Text,
        "application/xml"
        | "application/javascript"
        | "application/x-javascript"
        | "application/yaml"
        | "application/x-yaml"
        | "application/x-ndjson" => Media::Text,
        _ => Media::Other(essence.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tach_charset_khoi_content_type() {
        let parsed = ContentType::parse("text/HTML; charset=\"Windows-1252\"");
        assert_eq!(parsed.essence, "text/html");
        assert_eq!(parsed.charset.as_deref(), Some("windows-1252"));
        assert_eq!(parsed.media(), Media::Html);
    }

    #[test]
    fn khong_co_charset_thi_la_none() {
        let parsed = ContentType::parse("application/json");
        assert_eq!(parsed.charset, None);
        assert_eq!(parsed.media(), Media::Json);
    }

    #[test]
    fn hau_to_json_va_xml() {
        assert_eq!(classify("application/ld+json"), Media::Json);
        assert_eq!(classify("application/atom+xml"), Media::Text);
        assert_eq!(classify("text/csv"), Media::Text);
    }

    #[test]
    fn anh_khong_doc_duoc() {
        let media = classify("image/png");
        assert_eq!(media, Media::Other("image/png".to_string()));
        assert!(!media.readable());
    }
}
