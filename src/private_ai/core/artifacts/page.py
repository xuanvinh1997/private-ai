"""The HTML shell both the chart pages and the diagram pages are poured into.

One shell, so a chart and a system diagram produced in the same conversation look like
they came from the same tool. Everything is inline: a page written here has to open
correctly from a ``file://`` path on a machine with no network, which rules out an
external stylesheet or a webfont.
"""

from __future__ import annotations

import html
import json
from datetime import datetime

__all__ = ["escape", "js_literal", "render_page"]

escape = html.escape


def js_literal(value: object) -> str:
    """A Python value as JavaScript source, safe to drop inside a ``<script>``.

    ``</script>`` inside a string would close the tag early, and ``<!--`` opens an HTML
    comment; both are escaped as unicode so the JSON still parses to the same string.
    """
    text = json.dumps(value, ensure_ascii=False, default=str)
    return text.replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")


# Light palette on :root, dark redefined under prefers-color-scheme, plus an explicit
# toggle so the reader can override a system setting that fights the content.
_STYLE = """
:root {
  color-scheme: light dark;
  --bg: #f6f7f9;
  --surface: #ffffff;
  --border: #e2e5ea;
  --ink: #16191d;
  --muted: #667085;
  --accent: #2f6fd0;
  --grid: #eceef2;
  --up: #17915f;
  --down: #cf3f45;
  --shadow: 0 1px 2px rgba(16, 24, 40, .06), 0 8px 24px rgba(16, 24, 40, .06);
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #101317;
    --surface: #171b21;
    --border: #262c35;
    --ink: #e8ecf1;
    --muted: #98a2b3;
    --accent: #6fa8ff;
    --grid: #222831;
    --up: #35c48b;
    --down: #f0757b;
    --shadow: 0 1px 2px rgba(0, 0, 0, .4), 0 8px 24px rgba(0, 0, 0, .35);
  }
}
:root[data-theme="dark"] {
  --bg: #101317;
  --surface: #171b21;
  --border: #262c35;
  --ink: #e8ecf1;
  --muted: #98a2b3;
  --accent: #6fa8ff;
  --grid: #222831;
  --up: #35c48b;
  --down: #f0757b;
  --shadow: 0 1px 2px rgba(0, 0, 0, .4), 0 8px 24px rgba(0, 0, 0, .35);
}
* { box-sizing: border-box; }
html, body { margin: 0; }
body {
  background: var(--bg);
  color: var(--ink);
  font: 15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue",
        "Noto Sans", Arial, sans-serif;
  padding: 28px 20px 48px;
  -webkit-font-smoothing: antialiased;
}
.wrap { max-width: 1080px; margin: 0 auto; }
header { margin-bottom: 18px; display: flex; gap: 16px; align-items: flex-start; }
header .grow { flex: 1 1 auto; min-width: 0; }
h1 { font-size: 22px; line-height: 1.25; margin: 0; letter-spacing: -.01em; }
.subtitle { color: var(--muted); margin: 6px 0 0; font-size: 14px; }
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  box-shadow: var(--shadow);
  padding: 18px;
  overflow-x: auto;
}
footer { margin-top: 18px; color: var(--muted); font-size: 12.5px; }
footer p { margin: 4px 0; }
button.theme {
  flex: 0 0 auto;
  background: var(--surface);
  color: var(--muted);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 6px 12px;
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.theme:hover { color: var(--ink); border-color: var(--accent); }
pre.source {
  margin: 0;
  padding: 14px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 10px;
  overflow-x: auto;
  font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  color: var(--ink);
  white-space: pre;
}
details { margin-top: 14px; }
summary { cursor: pointer; color: var(--muted); font-size: 13.5px; }
.notice {
  margin: 0 0 12px;
  padding: 10px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--bg);
  color: var(--muted);
  font-size: 13px;
}
"""

# Remembering the choice is a per-reader convenience and nothing more, so a browser that
# refuses storage (private window, blocked site data) must still render the page.
_THEME_SCRIPT = """
(function () {
  var root = document.documentElement;
  var button = document.getElementById('theme-toggle');
  var stored = null;
  try { stored = localStorage.getItem('private-ai-artifact-theme'); } catch (e) {}
  if (stored === 'light' || stored === 'dark') root.setAttribute('data-theme', stored);
  function current() {
    var explicit = root.getAttribute('data-theme');
    if (explicit) return explicit;
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  if (!button) return;
  button.addEventListener('click', function () {
    var next = current() === 'dark' ? 'light' : 'dark';
    root.setAttribute('data-theme', next);
    try { localStorage.setItem('private-ai-artifact-theme', next); } catch (e) {}
    window.dispatchEvent(new Event('private-ai-theme'));
  });
})();
"""


def render_page(
    *,
    title: str,
    subtitle: str = "",
    body: str,
    style: str = "",
    script: str = "",
    head: str = "",
    source: str = "",
) -> str:
    """One complete standalone document.

    ``head`` is for the diagram page, which needs a module script the chart page does not.
    ``source`` is the citation line: where the numbers came from, in the author's words.
    """
    stamp = datetime.now().strftime("%d/%m/%Y %H:%M")
    footer = [f"<p>Tạo bởi Private AI · {escape(stamp)}</p>"]
    if source.strip():
        footer.insert(0, f"<p>Nguồn: {escape(source.strip())}</p>")
    subtitle_html = f'<p class="subtitle">{escape(subtitle)}</p>' if subtitle.strip() else ""
    return f"""<!doctype html>
<html lang="vi">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{escape(title)}</title>
<style>{_STYLE}{style}</style>
{head}
</head>
<body>
<div class="wrap">
<header>
  <div class="grow">
    <h1>{escape(title)}</h1>
    {subtitle_html}
  </div>
  <button class="theme" id="theme-toggle" type="button">Sáng / Tối</button>
</header>
{body}
<footer>{"".join(footer)}</footer>
</div>
<script>{_THEME_SCRIPT}</script>
<script>{script}</script>
</body>
</html>
"""
