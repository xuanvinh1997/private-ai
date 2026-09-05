//! HTML to Markdown, minus the furniture.
//!
//! Two crates would normally do this job -- one to strip the page, one to convert what is left --
//! but `dom_query` carries both halves, and it is already in this application's dependency graph
//! (Tauri's `wry` depends on it), so choosing `htmd` here would add a second HTML parser to the
//! build and buy nothing this crate needs. What remains is therefore a list of selectors and one
//! `md()` call, which is the whole point: the rules are data, and every one of them is testable.
//!
//! The order below matters and is not arbitrary. Furniture is removed *before* the body is chosen,
//! so a nav bar cannot make a `<div>` look like the article by sheer text volume; and links are
//! made absolute *before* serialising, because a relative `href` is worthless to a model that
//! cannot see the page it came from.

use dom_query::Document;
use url::Url;

/// Furniture, by tag.
///
/// `<header>` is deliberately absent: an article's own `<header>` usually holds the headline and
/// byline, which is exactly what must survive. Only the page-level banner is furniture, and that
/// one is caught by [`SITE_FURNITURE`] instead.
const FURNITURE_TAGS: &str = "script, style, noscript, template, iframe, object, embed, svg, \
                              canvas, form, nav, aside, footer, dialog, button, select, textarea, \
                              link, base";

/// The page's own banner and chrome, as opposed to an article's headline block. Matched only as a
/// direct child of `<body>`, which is what makes it distinguishable from the article's own header.
const SITE_FURNITURE: &str = "body > header, body > .header, body > #header, body > .site-header";

/// Tokens that mark a block as page furniture rather than content.
///
/// Matched against whole `-`/`_`/space-separated tokens of `class` and `id`, never as substrings:
/// `ad` as a substring also matches `download`, `header`, `read` and `thread`, and a substring
/// filter here eats the article it was meant to protect.
const JUNK_TOKENS: &[&str] = &[
    "ad",
    "ads",
    "adslot",
    "advert",
    "advertisement",
    "banner",
    "promo",
    "sponsor",
    "sponsored",
    "cookie",
    "cookiebar",
    "consent",
    "gdpr",
    "newsletter",
    "subscribe",
    "paywall",
    "modal",
    "popup",
    "lightbox",
    "sidebar",
    "navbar",
    "navigation",
    "breadcrumb",
    "breadcrumbs",
    "share",
    "sharing",
    "social",
    "related",
    "recommended",
    "comments",
    "disqus",
    "pagination",
    "skip-link",
    "toolbar",
];

/// Where the body of a page usually lives, most specific first. `body` is the floor: a page with
/// none of these still has to render something.
const CONTENT_ROOTS: &[&str] = &[
    "article",
    "main",
    "[role=main]",
    "#content",
    "#main",
    ".post-content",
    ".entry-content",
    ".article-body",
    "body",
];

/// A candidate root must hold at least this fraction of the page's remaining text, or it is a
/// teaser card rather than the article -- a "related stories" `<article>` would otherwise win on
/// nothing but being first in the document.
const MIN_ROOT_SHARE: usize = 4;

/// Attributes that hold a URL, and therefore need resolving against the page's own address.
const URL_ATTRS: &[(&str, &str)] = &[("a[href]", "href"), ("img[src]", "src")];

/// What survived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Article {
    /// The page's own title, if it has one worth reading.
    pub title: Option<String>,
    pub markdown: String,
}

/// Turn a whole HTML document into the Markdown of its body.
///
/// `base` is the address the HTML was fetched from; without it, relative links are dropped rather
/// than emitted, since a bare `/about` in a model's context is an invitation to fetch the wrong host.
pub fn to_markdown(html: &str, base: Option<&str>) -> Article {
    let doc = Document::from(html);

    // Read the title before stripping: `<title>` lives in `<head>`, which some of the rules below
    // are happy to take with them.
    let title = pick_title(&doc);

    if let Some(found) = doc.try_select(SITE_FURNITURE) {
        found.remove();
    }
    if let Some(found) = doc.try_select(FURNITURE_TAGS) {
        found.remove();
    }
    strip_junk(&doc);
    absolutize(&doc, base.and_then(|raw| Url::parse(raw).ok()).as_ref());

    Article {
        title,
        markdown: tidy(&pick_root(&doc)),
    }
}

/// Flatten an HTML fragment to its text. Search providers return snippets with `<strong>` marks
/// around the matched words; the marks are noise once the snippet is inside a Markdown list.
pub fn strip_tags(fragment: &str) -> String {
    let doc = Document::fragment(fragment);
    collapse_spaces(doc.root().text().as_ref())
}

fn pick_title(doc: &Document) -> Option<String> {
    for selector in ["head title", "title", "h1"] {
        let Some(found) = doc.try_select(selector) else {
            continue;
        };
        let text = collapse_spaces(found.text().as_ref());
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Drop elements whose `class` or `id` names them as furniture.
///
/// Done by walking every element that has either attribute rather than by a `[class*=...]`
/// selector, because substring matching is what makes these filters eat articles, and because a
/// selector list of forty attribute patterns is slower than one pass over the elements.
fn strip_junk(doc: &Document) {
    let Some(found) = doc.try_select("[class], [id]") else {
        return;
    };
    for node in found.nodes() {
        let named_junk = ["class", "id"].iter().any(|attr| {
            node.attr(attr)
                .map(|value| has_junk_token(value.as_ref()))
                .unwrap_or(false)
        });
        if named_junk {
            node.remove_from_parent();
        }
    }
}

fn has_junk_token(value: &str) -> bool {
    value
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .any(|token| {
            let token = token.to_ascii_lowercase();
            JUNK_TOKENS.contains(&token.as_str())
        })
}

/// Rewrite relative URLs against `base`, and delete every link whose scheme is not `http(s)`.
///
/// The deletion is the security half: `javascript:`, `data:` and `file:` links serialise into
/// Markdown as ordinary-looking links, and a model that later hands one back to a fetch tool has
/// been walked straight past the URL policy. Dropping the attribute keeps the anchor's text.
fn absolutize(doc: &Document, base: Option<&Url>) {
    for (selector, attr) in URL_ATTRS {
        let Some(found) = doc.try_select(selector) else {
            continue;
        };
        for node in found.nodes() {
            let Some(raw) = node.attr(attr) else { continue };
            let raw = raw.trim().to_string();
            // A bare fragment points inside a page the model cannot see; the text stays, the link goes.
            let resolved = if raw.is_empty() || raw.starts_with('#') {
                None
            } else {
                match base {
                    Some(base) => base.join(&raw).ok(),
                    None => Url::parse(&raw).ok(),
                }
            };
            match resolved.filter(|url| matches!(url.scheme(), "http" | "https")) {
                Some(url) => node.set_attr(attr, url.as_str()),
                None => node.remove_attr(attr),
            }
        }
    }
}

/// Choose the subtree that holds the article, and serialise it. Always answers with something:
/// an empty page is a fact about the page, not a reason to hand the caller an `Option` to unwrap.
fn pick_root(doc: &Document) -> String {
    let page_len = doc
        .try_select("body")
        .map(|body| body.text().chars().count())
        .unwrap_or(0);
    let floor = page_len / MIN_ROOT_SHARE;

    for selector in CONTENT_ROOTS {
        let Some(found) = doc.try_select(selector) else {
            continue;
        };
        let Some(node) = found.nodes().first() else {
            continue;
        };
        if node.text().chars().count() < floor {
            continue;
        }
        // `None` keeps `dom_query`'s own default skip list (script, style, meta, head); the tags
        // that matter to us are already gone from the tree by this point.
        return node.md(None).to_string();
    }
    // Nothing cleared the bar -- an all-boilerplate page, or one where the text lives loose in the
    // body. Serialising the whole document beats returning nothing.
    doc.md(None).to_string()
}

/// Squeeze the blank lines a stripped DOM leaves behind.
///
/// Removing a nav from between two paragraphs leaves the paragraph break plus the hole where the
/// nav was, and enough of those turn a page into mostly whitespace inside the token budget.
fn tidy(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut blanks = 0usize;
    for line in markdown.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blanks += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blanks > 0 { "\n\n" } else { "\n" });
        }
        blanks = 0;
        out.push_str(line);
    }
    out
}

fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page shaped like a real one: chrome above, article in the middle, furniture threaded
    /// through it, footer below.
    const TRANG_THAT: &str = r#"
<!doctype html>
<html lang="vi">
<head>
  <title>Cách nướng bánh mì — Bếp Nhà</title>
  <style>.hidden{display:none}</style>
  <script>window.dataLayer=[];track('pageview');</script>
</head>
<body>
  <header class="site-header"><a href="/">Bếp Nhà</a></header>
  <nav><ul><li><a href="/mon-chinh">Món chính</a></li><li><a href="/trang-mieng">Tráng miệng</a></li></ul></nav>
  <div class="ad-slot"><img src="/quang-cao.png" alt="Mua ngay"></div>
  <main>
    <article>
      <h1>Cách nướng bánh mì</h1>
      <p>Bánh mì ngon bắt đầu từ <strong>bột tốt</strong> và một lò đủ nóng.</p>
      <div class="social-share"><a href="/chia-se">Chia sẻ Facebook</a></div>
      <h2>Nguyên liệu</h2>
      <ul><li>500g bột mì</li><li>7g men</li></ul>
      <h2>Nhiệt độ</h2>
      <table>
        <thead><tr><th>Loại bánh</th><th>Nhiệt độ</th></tr></thead>
        <tbody><tr><td>Baguette</td><td>240°C</td></tr></tbody>
      </table>
      <pre><code class="language-bash">echo nuong</code></pre>
      <p>Xem thêm <a href="/men-no">bài về men nở</a> và <a href="javascript:alert(1)">mẹo nhỏ</a>.</p>
      <aside class="related"><a href="/banh-ngot">Bánh ngọt</a></aside>
    </article>
  </main>
  <footer><p>© 2026 Bếp Nhà. Mọi quyền được bảo lưu.</p></footer>
  <div id="disqus_thread">Bình luận của bạn đọc</div>
</body>
</html>
"#;

    #[test]
    fn rac_bien_mat() {
        let article = to_markdown(TRANG_THAT, Some("https://bepnha.example/bai/banh-mi"));
        let md = &article.markdown;
        assert!(!md.contains("dataLayer"), "script phải biến mất: {md}");
        assert!(!md.contains("display:none"), "style phải biến mất: {md}");
        assert!(!md.contains("Món chính"), "nav phải biến mất: {md}");
        assert!(!md.contains("Mua ngay"), "quảng cáo phải biến mất: {md}");
        assert!(!md.contains("Chia sẻ Facebook"), "nút chia sẻ phải biến mất: {md}");
        assert!(!md.contains("Mọi quyền được bảo lưu"), "footer phải biến mất: {md}");
        assert!(!md.contains("Bình luận của bạn đọc"), "khối bình luận phải biến mất: {md}");
        assert!(!md.contains("Bánh ngọt"), "aside phải biến mất: {md}");
    }

    #[test]
    fn than_bai_con_nguyen() {
        let article = to_markdown(TRANG_THAT, Some("https://bepnha.example/bai/banh-mi"));
        let md = &article.markdown;
        assert!(md.contains("Cách nướng bánh mì"), "{md}");
        assert!(md.contains("bột tốt"), "{md}");
        assert!(md.contains("500g bột mì"), "danh sách phải còn: {md}");
        assert!(md.contains("Baguette") && md.contains("240°C"), "bảng phải còn: {md}");
        assert!(md.contains("echo nuong"), "code block phải còn: {md}");
        assert!(md.contains("## Nguyên liệu"), "tiêu đề phải thành heading: {md}");
    }

    #[test]
    fn lay_duoc_tieu_de() {
        let article = to_markdown(TRANG_THAT, None);
        assert_eq!(
            article.title.as_deref(),
            Some("Cách nướng bánh mì — Bếp Nhà")
        );
    }

    #[test]
    fn link_tuong_doi_thanh_tuyet_doi() {
        let article = to_markdown(TRANG_THAT, Some("https://bepnha.example/bai/banh-mi"));
        assert!(
            article
                .markdown
                .contains("https://bepnha.example/men-no"),
            "{}",
            article.markdown
        );
    }

    #[test]
    fn link_javascript_bi_go_bo_nhung_giu_chu() {
        let article = to_markdown(TRANG_THAT, Some("https://bepnha.example/bai/banh-mi"));
        assert!(!article.markdown.contains("javascript:"), "{}", article.markdown);
        assert!(article.markdown.contains("mẹo nhỏ"), "{}", article.markdown);
    }

    #[test]
    fn khong_co_base_thi_bo_link_tuong_doi() {
        let article = to_markdown(
            r#"<body><main><p>Đọc <a href="/khac">bài khác</a> và <a href="https://vidu.test/x">bài xa</a>.</p></main></body>"#,
            None,
        );
        assert!(!article.markdown.contains("/khac"), "{}", article.markdown);
        assert!(article.markdown.contains("bài khác"), "{}", article.markdown);
        assert!(
            article.markdown.contains("https://vidu.test/x"),
            "{}",
            article.markdown
        );
    }

    #[test]
    fn the_article_nho_khong_cuop_duoc_than_bai() {
        // A teaser `<article>` first, the real body after it: the share test is what stops the teaser.
        let html = format!(
            r#"<body><article><p>Tin ngắn.</p></article><main><p>{}</p></main></body>"#,
            "Nội dung thật. ".repeat(60)
        );
        let article = to_markdown(&html, None);
        assert!(article.markdown.contains("Nội dung thật"), "{}", article.markdown);
    }

    #[test]
    fn go_the_khoi_doan_trich() {
        assert_eq!(
            strip_tags("Kết quả <strong>tìm</strong> được\n  hôm nay"),
            "Kết quả tìm được hôm nay"
        );
    }

    #[test]
    fn khong_de_lai_ba_dong_trong_lien_tiep() {
        let article = to_markdown(TRANG_THAT, None);
        assert!(!article.markdown.contains("\n\n\n"), "{}", article.markdown);
    }
}
