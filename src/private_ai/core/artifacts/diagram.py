"""Mermaid diagrams as a standalone HTML page.

Mermaid is a compiler, not a drawing: rendering the source needs mermaid.js, and the
bundle is a few megabytes. Vendoring that into the repository to draw the occasional box
and arrow is not a trade worth making, so the page loads it from a CDN *when the machine
happens to be online* and degrades to the source text when it is not.

That fallback is the point rather than an apology. Mermaid source is readable — a person
can follow ``A --> B`` without a renderer — so an offline reader still gets the diagram's
content, and the ``.mmd`` file written alongside opens in any Mermaid-aware editor.
"""

from __future__ import annotations

import re
from collections.abc import Sequence
from dataclasses import dataclass, field

from private_ai.core.artifacts.page import escape, js_literal, render_page
from private_ai.core.artifacts.store import ArtifactError

__all__ = ["MERMAID_CDN", "DiagramSpec", "render_diagram_page"]

MERMAID_CDN = "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs"

# Every diagram Mermaid 11 understands. The check is on the first meaningful line only:
# validating the body would mean reimplementing the parser, which is what the browser is
# for. This catches the actual failure mode — prose, or a fenced block pasted whole.
DIAGRAM_KEYWORDS = (
    "architecture-beta",
    "block-beta",
    "c4component",
    "c4container",
    "c4context",
    "c4deployment",
    "c4dynamic",
    "classdiagram",
    "erdiagram",
    "flowchart",
    "gantt",
    "gitgraph",
    "graph",
    "journey",
    "kanban",
    "mindmap",
    "packet-beta",
    "pie",
    "quadrantchart",
    "radar-beta",
    "requirementdiagram",
    "sankey-beta",
    "sequencediagram",
    "statediagram",
    "statediagram-v2",
    "timeline",
    "treemap-beta",
    "xychart-beta",
    "zenuml",
)

MAX_SOURCE_CHARS = 40_000

_DIRECTIVE = re.compile(r"\A\s*%%\{.*?\}%%", re.S)
_FRONTMATTER = re.compile(r"\A\s*---[ \t]*\r?\n.*?\r?\n---[ \t]*(?:\r?\n|\Z)", re.S)
_FENCE = re.compile(r"\A\s*```(?:mermaid)?[ \t]*\r?\n(?P<body>.*?)\r?\n?```\s*\Z", re.S)


def _strip_fence(source: str) -> str:
    """Accept a fenced block and unwrap it: a model that pastes one meant the contents."""
    matched = _FENCE.match(source)
    return matched.group("body") if matched else source


def _first_statement(source: str) -> str:
    body = _FRONTMATTER.sub("", source, count=1)
    while True:
        stripped = _DIRECTIVE.sub("", body, count=1)
        if stripped == body:
            break
        body = stripped
    for line in body.splitlines():
        text = line.strip()
        if text and not text.startswith("%%"):
            return text
    return ""


@dataclass(frozen=True, slots=True)
class DiagramSpec:
    title: str
    source: str
    subtitle: str = ""
    caption: str = ""
    notes: Sequence[str] = field(default=())

    def cleaned(self) -> str:
        return _strip_fence(self.source).strip()

    def validate(self) -> str:
        """Return the source to render, or explain what is wrong with it."""
        if not self.title.strip():
            raise ArtifactError("Thiếu 'title': sơ đồ cần một tiêu đề.")
        source = self.cleaned()
        if not source:
            raise ArtifactError("Thiếu 'source': cần mã Mermaid cho sơ đồ.")
        if len(source) > MAX_SOURCE_CHARS:
            raise ArtifactError(
                f"Mã Mermaid dài {len(source)} ký tự, vượt mức {MAX_SOURCE_CHARS}. "
                "Tách thành nhiều sơ đồ nhỏ hơn."
            )
        head = _first_statement(source).casefold()
        if not any(head.startswith(keyword) for keyword in DIAGRAM_KEYWORDS):
            raise ArtifactError(
                "Dòng đầu tiên phải khai báo loại sơ đồ Mermaid, ví dụ 'flowchart TB', "
                "'sequenceDiagram' hay 'erDiagram'. "
                f"Hiện đang bắt đầu bằng: {_first_statement(source)[:60]!r}"
            )
        return source


_DIAGRAM_STYLE = """
#diagram { display: flex; justify-content: center; min-height: 80px; }
#diagram svg { max-width: 100%; height: auto; }
#diagram .loading { color: var(--muted); font-size: 13.5px; padding: 24px 0; }
figcaption { color: var(--muted); font-size: 13.5px; margin-top: 12px; text-align: center; }
.notes { margin: 14px 0 0; padding-left: 20px; color: var(--muted); font-size: 13.5px; }
.notes li { margin: 3px 0; }
button.copy {
  background: transparent; border: 1px solid var(--border); border-radius: 8px;
  color: var(--muted); font: inherit; font-size: 12.5px; padding: 4px 10px;
  cursor: pointer; margin: 10px 0;
}
button.copy:hover { color: var(--ink); border-color: var(--accent); }
"""

_DIAGRAM_SCRIPT = """
const CDN = %(cdn)s;
const source = %(source)s;
const host = document.getElementById('diagram');
const notice = document.getElementById('offline');
const details = document.getElementById('source-details');
// A CDN that is merely unreachable fails fast; one that black-holes the request does not,
// and a page stuck on "đang dựng" forever is worse than one that admits it is offline.
const LOAD_TIMEOUT_MS = 12000;
let mermaid = null;
let counter = 0;

function dark() {
  const explicit = document.documentElement.getAttribute('data-theme');
  if (explicit) return explicit === 'dark';
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

// Imported dynamically, not with a static `import` at the top: a static one that cannot
// be fetched aborts the whole module, so the catch below would never run and the reader
// would be left staring at the loading line with no explanation.
async function library() {
  if (mermaid) return mermaid;
  const loaded = await Promise.race([
    import(CDN),
    new Promise((_, reject) => setTimeout(
      () => reject(new Error('quá hạn tải thư viện Mermaid')), LOAD_TIMEOUT_MS))
  ]);
  mermaid = loaded.default || loaded;
  return mermaid;
}

async function render() {
  const engine = await library();
  engine.initialize({
    startOnLoad: false,
    // 'strict' keeps any HTML inside a node label as text rather than markup. The source
    // is authored locally, but the page may be forwarded to someone who did not write it.
    securityLevel: 'strict',
    theme: dark() ? 'dark' : 'default',
    fontFamily: 'inherit'
  });
  const { svg } = await engine.render('mermaid-' + (counter++), source);
  host.innerHTML = svg;
}

function fail(error) {
  host.innerHTML = '';
  if (notice) {
    notice.hidden = false;
    notice.textContent = 'Không vẽ được sơ đồ (' +
      ((error && error.message) || 'không tải được thư viện Mermaid') +
      '). Mã nguồn bên dưới vẫn đọc được nguyên vẹn.';
  }
  if (details) details.open = true;
}

render().then(function () {
  if (notice) notice.hidden = true;
}).catch(fail);

function rerender() { if (mermaid) render().catch(fail); }
window.addEventListener('private-ai-theme', rerender);
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', rerender);

const copy = document.getElementById('copy');
if (copy) {
  copy.addEventListener('click', async function () {
    try {
      await navigator.clipboard.writeText(source);
      copy.textContent = 'Đã sao chép';
    } catch (error) {
      copy.textContent = 'Không sao chép được';
    }
    setTimeout(function () { copy.textContent = 'Sao chép mã Mermaid'; }, 1800);
  });
}
"""

# The library is loaded from the network, so an unreachable CDN must not read as a crash.
_LOADING_TEXT = "Đang dựng sơ đồ…"


def render_diagram_page(spec: DiagramSpec) -> str:
    source = spec.validate()
    caption = f"<figcaption>{escape(spec.caption)}</figcaption>" if spec.caption.strip() else ""
    notes = ""
    if spec.notes:
        items = "".join(f"<li>{escape(note)}</li>" for note in spec.notes)
        notes = f'<ul class="notes">{items}</ul>'
    script = _DIAGRAM_SCRIPT % {"cdn": js_literal(MERMAID_CDN), "source": js_literal(source)}
    body = (
        '<div class="card">'
        f'<p class="notice" id="offline">{_LOADING_TEXT}</p>'
        '<figure style="margin:0">'
        '<div id="diagram"></div>'
        f"{caption}"
        "</figure>"
        f"{notes}"
        "</div>"
        '<details id="source-details">'
        "<summary>Mã nguồn Mermaid</summary>"
        '<button class="copy" id="copy" type="button">Sao chép mã Mermaid</button>'
        f'<pre class="source">{escape(source)}</pre>'
        "</details>"
        f'<script type="module">{script}</script>'
    )
    return render_page(
        title=spec.title,
        subtitle=spec.subtitle,
        body=body,
        style=_DIAGRAM_STYLE,
    )
