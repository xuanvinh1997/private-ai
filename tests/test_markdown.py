"""The UI's Markdown renderer.

Model output is untrusted and there is no DOMPurify here. The safety property is the
opposite one: nothing in the source text ever reaches the output as markup. Every test
below is a way of trying to break that.
"""

from __future__ import annotations

import re

import pytest

from private_ai.ui.markdown import document_css, escape_html, markdown_to_html, plain_text

TOKENS = {
    "text": "#111111",
    "ink": "#000000",
    "accent": "#176b59",
    "accent-ink": "#0d4a3e",
    "accent-soft": "#e3f0ec",
    "surface-soft": "#f3f6f4",
    "line": "#dddddd",
    "line-strong": "#bbbbbb",
    "muted": "#666666",
    "faint": "#999999",
}


# --- escaping -------------------------------------------------------------


@pytest.mark.parametrize(
    "source",
    [
        "<script>alert(1)</script>",
        "<iframe src=x></iframe>",
        "<svg onload=alert(1)>",
        "<div onclick='steal()'>bấm</div>",
        "<!-- <script>alert(1)</script> -->",
        "<body onload=alert(1)>",
    ],
)
def test_no_untrusted_tag_survives_as_markup(source: str) -> None:
    rendered = markdown_to_html(source)
    assert "<script" not in rendered.lower()
    assert "<iframe" not in rendered.lower()
    assert "<svg" not in rendered.lower()
    assert "onload" not in rendered.lower() or "&lt;" in rendered
    # The characters are still there, just as text.
    assert "&lt;" in rendered


def test_a_script_tag_renders_as_the_words_it_is() -> None:
    assert markdown_to_html("<script>alert(1)</script>") == (
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"
    )


def test_images_are_dropped_and_only_the_alt_text_survives() -> None:
    """The old DOMPurify config forbade ``img``; so does this, from the other direction."""
    assert markdown_to_html("![biểu đồ](http://evil/p.gif)") == "<p>biểu đồ</p>"
    assert "<img" not in markdown_to_html('<img src="x" onerror="alert(1)">').lower()


def test_style_tags_and_style_attributes_are_stripped() -> None:
    assert "<style" not in markdown_to_html("<style>body{display:none}</style>").lower()
    rendered = markdown_to_html('[nhãn](https://example.com "tiêu đề")')
    assert "style=" not in rendered
    # Text that merely talks about a style attribute keeps its words.
    prose = markdown_to_html('Đặt style="color:red" vào thẻ')
    assert "color:red" in prose


@pytest.mark.parametrize(
    "href",
    [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "java\tscript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "vbscript:msgbox(1)",
        "file:///etc/passwd",
    ],
)
def test_only_safe_schemes_become_links(href: str) -> None:
    rendered = markdown_to_html(f"[đi]({href})")
    assert "<a " not in rendered
    assert "đi" in rendered


@pytest.mark.parametrize(
    "href",
    ["https://example.com/a", "http://example.com", "mailto:ai@example.com", "#muc-1", "/tai-lieu"],
)
def test_safe_schemes_do_become_links(href: str) -> None:
    rendered = markdown_to_html(f"[đi]({href})")
    assert f'<a href="{href}">đi</a>' in rendered


def test_a_link_label_cannot_smuggle_markup() -> None:
    rendered = markdown_to_html("[<b>đậm</b>](https://example.com)")
    assert "<b>" not in rendered
    assert "&lt;b&gt;" in rendered


def test_the_placeholder_sentinels_cannot_be_forged() -> None:
    """A crafted NUL would otherwise let input point at markup this module parked."""
    rendered = markdown_to_html("\x00 0 \x00 và \x01xin chào\x01")
    assert "\x00" not in rendered
    assert "\x01" not in rendered


def test_escape_html_quotes_attributes_too() -> None:
    assert escape_html('<a href="x">') == "&lt;a href=&quot;x&quot;&gt;"
    assert escape_html("") == ""


# --- rendering ------------------------------------------------------------


def test_headings_bold_italic_and_inline_code() -> None:
    assert markdown_to_html("# Tiêu đề") == "<h1>Tiêu đề</h1>"
    assert markdown_to_html("### Nhỏ hơn") == "<h3>Nhỏ hơn</h3>"
    assert markdown_to_html("**a** và *b*") == "<p><strong>a</strong> và <em>b</em></p>"
    assert markdown_to_html("~~bỏ~~") == "<p><s>bỏ</s></p>"
    # Content inside a code span is escaped and never re-processed.
    assert markdown_to_html("`b<c>`") == "<p><code>b&lt;c&gt;</code></p>"
    assert markdown_to_html("`**không đậm**`") == "<p><code>**không đậm**</code></p>"


def test_a_fenced_code_block_keeps_its_language_and_escapes_its_body() -> None:
    rendered = markdown_to_html("```python\nprint('<a>')\n```")
    assert rendered == "<pre><code class=\"language-python\">print('&lt;a&gt;')</code></pre>"

    tilde = markdown_to_html("~~~\nnội dung\n~~~")
    assert tilde == "<pre><code>nội dung</code></pre>"


def test_a_code_fence_language_cannot_carry_an_attribute() -> None:
    """The class is the only place a fence's own text reaches an HTML attribute."""
    rendered = markdown_to_html("```py onload=alert(1)\nnội dung\n```")
    assert "<code" in rendered
    for tag in re.findall(r"<code[^>]*>", rendered):
        assert "onload" not in tag
        assert re.fullmatch(r'<code(?: class="language-[a-zA-Z0-9_+-]*")?>', tag)


def test_unordered_and_ordered_lists_including_nesting() -> None:
    assert markdown_to_html("- một\n- hai") == "<ul><li>một</li><li>hai</li></ul>"
    assert markdown_to_html("1. một\n2. hai") == "<ol><li>một</li><li>hai</li></ol>"

    nested = markdown_to_html("- ngoài\n  - trong")
    assert nested == "<ul><li>ngoài</li><ul><li>trong</li></ul></ul>"


def test_a_table_renders_with_its_alignments() -> None:
    rendered = markdown_to_html("| Tên | Số |\n|:---|---:|\n| a | 1 |\n| b | 2 |")
    assert rendered.startswith("<table><thead><tr>")
    assert '<th align="left">Tên</th>' in rendered
    assert '<th align="right">Số</th>' in rendered
    assert '<td align="left">a</td>' in rendered
    assert rendered.count("<tr>") == 3


def test_pipes_without_a_delimiter_row_are_just_text() -> None:
    rendered = markdown_to_html("a | b\nc | d")
    assert "<table" not in rendered
    assert "<p>" in rendered


def test_blockquotes_rules_and_paragraph_joining() -> None:
    assert markdown_to_html("> trích dẫn") == "<blockquote><p>trích dẫn</p></blockquote>"
    assert markdown_to_html("---") == "<hr/>"
    assert markdown_to_html("dòng một\ndòng hai") == "<p>dòng một dòng hai</p>"
    assert markdown_to_html("một\n\nhai") == "<p>một</p><p>hai</p>"


def test_carriage_returns_and_empty_input() -> None:
    assert markdown_to_html("") == ""
    assert markdown_to_html("một\r\nhai") == "<p>một hai</p>"


def test_plain_text_hands_back_the_markdown_source() -> None:
    """Copy gives the source, so the renderer and the clipboard cannot disagree."""
    source = "**đậm**\r\n`mã`"
    assert plain_text(source) == "**đậm**\n`mã`"
    assert plain_text("") == ""


def test_document_css_is_built_from_the_theme_tokens() -> None:
    css = document_css(TOKENS)
    assert TOKENS["accent"] in css
    assert TOKENS["surface-soft"] in css
    # Qt's rich-text engine understands only a small CSS subset.
    for unsupported in ("flex", "--", "rem", "grid-template"):
        assert unsupported not in css

    scaled = document_css(TOKENS, {"sm": 15, "base": 18, "md": 19, "lg": 22, "xl": 26})
    assert "font-size: 18px" in scaled


# --- inside the widget that actually shows it ----------------------------


def test_qtextbrowser_renders_the_fragment_without_executing_it(qapp: object) -> None:
    """The renderer's output is only safe if Qt reads it the way we intended."""
    from PySide6.QtWidgets import QTextBrowser

    from private_ai.ui.theme import LIGHT, type_scale

    browser = QTextBrowser()
    browser.document().setDefaultStyleSheet(document_css(LIGHT, type_scale("normal")))
    browser.setHtml(markdown_to_html("# Tiêu đề\n\n<script>alert(1)</script>\n\n- một"))

    shown = browser.toPlainText()
    assert "Tiêu đề" in shown
    # The script survives as visible text rather than as a tag Qt would strip silently.
    assert "<script>alert(1)</script>" in shown
    assert "một" in shown
    browser.deleteLater()
