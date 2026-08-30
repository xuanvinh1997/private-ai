"""A deliberately small Markdown renderer for assistant output.

Model output is untrusted. The old build ran ``marked`` and then DOMPurify with
``FORBID_TAGS: ["img", "style"]`` / ``FORBID_ATTR: ["style"]``; there is no DOMPurify here,
so the safety comes from the opposite direction: **nothing in the source text ever reaches
the output as markup**. Every character is HTML-escaped first, and tags only ever appear
because this module emitted them. That makes the sanitiser unnecessary rather than
optional, and it is why the renderer must never grow a "raw HTML passthrough" mode.

Consequences worth stating, because they are the test cases:

>>> markdown_to_html("<script>alert(1)</script>")
'<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>'
>>> markdown_to_html("![x](http://evil/p.gif)")           # images are dropped entirely
'<p>x</p>'
>>> markdown_to_html("[go](javascript:alert(1))")         # only http/https/mailto survive
'<p>go</p>'
>>> markdown_to_html("**a** `b<c>`")
'<p><strong>a</strong> <code>b&lt;c&gt;</code></p>'

Supported: ATX headings, ``**bold**``/``__bold__``, ``*italic*``/``_italic_``,
``~~strike~~``, inline code, fenced and indent-free code blocks, unordered and ordered
lists (nested by two-space indent), links, blockquotes, horizontal rules and GFM tables.
Anything else degrades to escaped text, which is the correct failure mode.
"""

from __future__ import annotations

import html
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

__all__ = ["document_css", "escape_html", "markdown_to_html", "plain_text"]

_SAFE_SCHEMES = ("http://", "https://", "mailto:")

_FENCE = re.compile(r"^\s*(`{3,}|~{3,})\s*([\w+-]*)\s*$")
_HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
_RULE = re.compile(r"^\s*(?:-{3,}|\*{3,}|_{3,})\s*$")
_QUOTE = re.compile(r"^\s{0,3}>\s?(.*)$")
_ULIST = re.compile(r"^(\s*)[-*+]\s+(.*)$")
_OLIST = re.compile(r"^(\s*)(\d{1,9})[.)]\s+(.*)$")
_TABLE_SEP = re.compile(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$")

# Inline pattern order matters: code first so nothing inside a span is re-processed,
# then images (dropped), then links, then emphasis longest-marker-first.
_IMAGE = re.compile(r"!\[([^\]]*)\]\(\s*<?([^)\s]*)>?(?:\s+\"[^\"]*\")?\s*\)")
_LINK = re.compile(r"\[([^\]]*)\]\(\s*<?([^)\s]*)>?(?:\s+\"[^\"]*\")?\s*\)")
_AUTOLINK = re.compile(r"<((?:https?://|mailto:)[^>\s]+)>")
_CODE_SPAN = re.compile(r"(`+)(?!`)(.+?)(?<!`)\1(?!`)", re.DOTALL)
_STRONG = re.compile(r"(?<!\w)(\*\*|__)(?=\S)(.+?)(?<=\S)\1(?!\w)", re.DOTALL)
_EM = re.compile(r"(?<![\w*_])([*_])(?=[^\s*_])(.+?)(?<=[^\s*_])\1(?![\w*_])", re.DOTALL)
_STRIKE = re.compile(r"~~(?=\S)(.+?)(?<=\S)~~", re.DOTALL)

# Defence in depth: even though nothing unescaped can reach the output, the final string
# is swept for these two so a future edit that forgets the rule still cannot ship them.
_IMG_TAG = re.compile(r"<\s*/?\s*img\b[^>]*>", re.IGNORECASE)
_STYLE_TAG = re.compile(r"<\s*style\b[^>]*>.*?<\s*/\s*style\s*>", re.IGNORECASE | re.DOTALL)
_STYLE_ATTR = re.compile(r"\sstyle\s*=\s*(\"[^\"]*\"|'[^']*'|[^\s>]+)", re.IGNORECASE)


def escape_html(text: str) -> str:
    return html.escape(text or "", quote=True)


def _safe_href(raw: str) -> str:
    """Only three schemes survive; everything else (``javascript:``, ``data:``, ``file:``)
    yields "" and the caller renders the link text alone."""
    url = (raw or "").strip()
    if not url:
        return ""
    lowered = url.lower()
    # A control character or whitespace inside the scheme is the classic
    # "java\tscript:" bypass — reject rather than try to normalise it.
    if any(ord(ch) < 0x20 or ch.isspace() for ch in url):
        return ""
    if lowered.startswith(_SAFE_SCHEMES):
        return html.escape(url, quote=True)
    # Relative and anchor targets are harmless and occasionally useful in citations.
    if lowered.startswith(("/", "#", "./", "../")) or (":" not in lowered.split("/")[0]):
        return html.escape(url, quote=True)
    return ""


def _inline(text: str) -> str:
    """Escape first, then re-introduce the handful of tags we choose to emit."""
    if not text:
        return ""
    placeholders: list[str] = []

    def _stash(markup: str) -> str:
        placeholders.append(markup)
        return f"\x00{len(placeholders) - 1}\x00"

    # 1. Code spans: capture before escaping so backticks survive, escape the content,
    #    and park the result where no later pattern can see it.
    def _code(match: re.Match[str]) -> str:
        body = match.group(2)
        if body.startswith(" ") and body.endswith(" ") and body.strip():
            body = body[1:-1]
        return _stash(f"<code>{html.escape(body, quote=False)}</code>")

    working = _CODE_SPAN.sub(_code, text)

    # 2. Images are removed outright; the alt text is kept as plain words.
    working = _IMAGE.sub(lambda m: m.group(1), working)

    def _link(match: re.Match[str]) -> str:
        label = match.group(1)
        href = _safe_href(match.group(2))
        if not href:
            return label
        return _stash(f'<a href="{href}">\x01{label}\x01</a>')

    working = _LINK.sub(_link, working)

    def _auto(match: re.Match[str]) -> str:
        href = _safe_href(match.group(1))
        return _stash(f'<a href="{href}">\x01{match.group(1)}\x01</a>') if href else ""

    working = _AUTOLINK.sub(_auto, working)

    # 3. Now escape everything that is still source text.
    working = html.escape(working, quote=False)

    # 4. Emphasis, on escaped text: the markers themselves are ASCII and untouched by
    #    escaping, so the patterns still line up.
    working = _STRONG.sub(lambda m: f"<strong>{m.group(2)}</strong>", working)
    working = _EM.sub(lambda m: f"<em>{m.group(2)}</em>", working)
    working = _STRIKE.sub(lambda m: f"<s>{m.group(1)}</s>", working)

    # 5. Put the parked markup back, escaping any link label that travelled with it.
    def _restore(match: re.Match[str]) -> str:
        markup = placeholders[int(match.group(1))]
        return re.sub(r"\x01(.*?)\x01", lambda m: html.escape(m.group(1), quote=False), markup)

    working = re.sub(r"\x00(\d+)\x00", _restore, working)
    return working.replace("\x00", "").replace("\x01", "")


def _code_block(lines: Sequence[str], language: str) -> str:
    body = html.escape("\n".join(lines), quote=False)
    cls = f' class="language-{re.sub(r"[^a-zA-Z0-9_+-]", "", language)}"' if language else ""
    return f"<pre><code{cls}>{body}</code></pre>"


def _table(rows: list[list[str]], alignments: list[str]) -> str:
    def cells(values: list[str], tag: str) -> str:
        out = []
        for index, value in enumerate(values):
            align = alignments[index] if index < len(alignments) else ""
            attr = f' align="{align}"' if align else ""
            out.append(f"<{tag}{attr}>{_inline(value)}</{tag}>")
        return "".join(out)

    head = f"<thead><tr>{cells(rows[0], 'th')}</tr></thead>" if rows else ""
    body = "".join(f"<tr>{cells(row, 'td')}</tr>" for row in rows[1:])
    return f"<table>{head}<tbody>{body}</tbody></table>"


def _split_row(line: str) -> list[str]:
    stripped = line.strip()
    if stripped.startswith("|"):
        stripped = stripped[1:]
    if stripped.endswith("|"):
        stripped = stripped[:-1]
    # Split on pipes that are not escaped.
    return [cell.strip() for cell in re.split(r"(?<!\\)\|", stripped)]


def _alignments(separator: str) -> list[str]:
    out = []
    for cell in _split_row(separator):
        left, right = cell.startswith(":"), cell.endswith(":")
        out.append("center" if left and right else "right" if right else "left" if left else "")
    return out


class _ListBuilder:
    """Builds nested ``<ul>``/``<ol>`` from indent depth, closing what the next line ends."""

    def __init__(self) -> None:
        self.stack: list[tuple[int, str]] = []
        self.parts: list[str] = []

    def item(self, indent: int, ordered: bool, text: str) -> None:
        depth = indent // 2
        tag = "ol" if ordered else "ul"
        while self.stack and (
            self.stack[-1][0] > depth or (self.stack[-1][0] == depth and self.stack[-1][1] != tag)
        ):
            self.parts.append(f"</{self.stack.pop()[1]}>")
        if not self.stack or self.stack[-1][0] < depth:
            self.stack.append((depth, tag))
            self.parts.append(f"<{tag}>")
        self.parts.append(f"<li>{_inline(text)}</li>")

    def close(self) -> str:
        while self.stack:
            self.parts.append(f"</{self.stack.pop()[1]}>")
        out = "".join(self.parts)
        self.parts = []
        return out


def _render_blocks(lines: Sequence[str]) -> str:  # noqa: C901 - a block scanner is a switch
    out: list[str] = []
    paragraph: list[str] = []
    lists = _ListBuilder()
    index = 0
    total = len(lines)

    def flush_paragraph() -> None:
        if paragraph:
            out.append(f"<p>{_inline(' '.join(paragraph))}</p>")
            paragraph.clear()

    def flush_list() -> None:
        rendered = lists.close()
        if rendered:
            out.append(rendered)

    while index < total:
        line = lines[index].rstrip()

        fence = _FENCE.match(line)
        if fence:
            flush_paragraph()
            flush_list()
            marker = fence.group(1)[0]
            body: list[str] = []
            index += 1
            while index < total:
                closing = _FENCE.match(lines[index].rstrip())
                if closing and closing.group(1)[0] == marker:
                    index += 1
                    break
                body.append(lines[index])
                index += 1
            out.append(_code_block(body, fence.group(2)))
            continue

        if not line.strip():
            flush_paragraph()
            flush_list()
            index += 1
            continue

        heading = _HEADING.match(line)
        if heading:
            flush_paragraph()
            flush_list()
            level = len(heading.group(1))
            out.append(f"<h{level}>{_inline(heading.group(2).rstrip('#').strip())}</h{level}>")
            index += 1
            continue

        if _RULE.match(line) and not _ULIST.match(line):
            flush_paragraph()
            flush_list()
            out.append("<hr/>")
            index += 1
            continue

        if _QUOTE.match(line):
            flush_paragraph()
            flush_list()
            quoted: list[str] = []
            while index < total and (match := _QUOTE.match(lines[index].rstrip())):
                quoted.append(match.group(1))
                index += 1
            out.append(f"<blockquote>{_render_blocks(quoted)}</blockquote>")
            continue

        # A GFM table needs its delimiter row on the very next line, otherwise the
        # pipes are just text.
        if "|" in line and index + 1 < total and _TABLE_SEP.match(lines[index + 1]):
            flush_paragraph()
            flush_list()
            alignments = _alignments(lines[index + 1])
            rows = [_split_row(line)]
            index += 2
            while index < total and "|" in lines[index] and lines[index].strip():
                rows.append(_split_row(lines[index]))
                index += 1
            out.append(_table(rows, alignments))
            continue

        unordered = _ULIST.match(line)
        ordered = _OLIST.match(line)
        if unordered or ordered:
            flush_paragraph()
            if unordered is not None:
                lists.item(len(unordered.group(1)), False, unordered.group(2))
            elif ordered is not None:
                lists.item(len(ordered.group(1)), True, ordered.group(3))
            index += 1
            continue

        flush_list()
        paragraph.append(line.strip())
        index += 1

    flush_paragraph()
    flush_list()
    return "".join(out)


def _strip_forbidden(markup: str) -> str:
    markup = _STYLE_TAG.sub("", markup)
    markup = _IMG_TAG.sub("", markup)
    # Scoped to real tags. Body text has already had its angle brackets escaped, so every
    # ``<...>`` left in the string is markup this module emitted — without that scoping,
    # a message that merely *talks about* ``style="…"`` would lose the words.
    return re.sub(r"<[^>]*>", lambda m: _STYLE_ATTR.sub("", m.group(0)), markup)


def markdown_to_html(text: str) -> str:
    """Render untrusted Markdown to an HTML fragment safe for ``QTextBrowser``."""
    if not text:
        return ""
    normalized = text.replace("\r\n", "\n").replace("\r", "\n")
    # NUL and our two sentinels would otherwise let crafted input forge a placeholder.
    normalized = normalized.replace("\x00", "").replace("\x01", "")
    return _strip_forbidden(_render_blocks(normalized.split("\n")))


def plain_text(text: str) -> str:
    """The clipboard wants the Markdown source, not the rendering — kept here so the
    "Copy" action and the renderer cannot disagree about what a message is."""
    return (text or "").replace("\r\n", "\n").replace("\r", "\n")


def document_css(tokens: dict[str, str], sizes: dict[str, int] | None = None) -> str:
    """A default stylesheet for ``QTextBrowser.document()``.

    Qt's rich-text engine understands a small CSS subset only; everything here is inside
    it (no flexbox, no custom properties, no ``rem``).
    """
    fs = sizes or {"sm": 13, "base": 14, "md": 15, "lg": 17, "xl": 20}
    return f"""
    body {{ color: {tokens["text"]}; font-size: {fs["base"]}px; }}
    p {{ margin: 0 0 10px 0; line-height: 150%; }}
    h1, h2, h3, h4, h5, h6 {{ color: {tokens["ink"]}; margin: 14px 0 7px 0; }}
    h1 {{ font-size: {fs["xl"]}px; }}
    h2 {{ font-size: {fs["lg"]}px; }}
    h3, h4, h5, h6 {{ font-size: {fs["md"]}px; }}
    a {{ color: {tokens["accent"]}; text-decoration: none; }}
    code {{
        font-family: "IBM Plex Mono", "SF Mono", Consolas, monospace;
        font-size: {fs["sm"]}px;
        color: {tokens["accent-ink"]};
        background-color: {tokens["accent-soft"]};
    }}
    pre {{
        margin: 10px 0;
        padding: 10px 12px;
        color: {tokens["ink"]};
        background-color: {tokens["surface-soft"]};
        border: 1px solid {tokens["line"]};
        font-family: "IBM Plex Mono", "SF Mono", Consolas, monospace;
        font-size: {fs["sm"]}px;
    }}
    pre code {{ background-color: transparent; color: {tokens["ink"]}; }}
    blockquote {{
        margin: 10px 0 10px 4px;
        padding-left: 12px;
        border-left: 3px solid {tokens["line-strong"]};
        color: {tokens["muted"]};
    }}
    ul, ol {{ margin: 6px 0 10px 0; }}
    li {{ margin: 3px 0; }}
    table {{ border-collapse: collapse; margin: 10px 0; }}
    th, td {{ border: 1px solid {tokens["line"]}; padding: 6px 10px; }}
    th {{ background-color: {tokens["surface-soft"]}; color: {tokens["ink"]}; }}
    hr {{ border: 0; border-top: 1px solid {tokens["line"]}; }}
    s {{ color: {tokens["faint"]}; }}
    """
