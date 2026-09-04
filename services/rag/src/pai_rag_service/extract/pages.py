"""Page boundary markers, so a citation can say which page it came from.
An HTML comment, because it must survive Markdown unseen; chunking turns it into `page`.
"""

from __future__ import annotations

import re

__all__ = ["PAGE_MARKER", "mark", "split_pages", "strip_markers"]

#: Matches a line containing only the marker and nothing else.
PAGE_MARKER = re.compile(r"^<!--\s*pai-page:(\d+)\s*-->$")


def mark(page: int) -> str:
    """Marker opening a page. 1-based - the number the user sees on the PDF."""
    return f"<!-- pai-page:{page} -->"


def split_pages(pages: list[str]) -> str:
    """Join the pages, each opening with its own marker."""
    parts: list[str] = []
    for index, body in enumerate(pages, start=1):
        parts.append(mark(index))
        parts.append(body.strip())
    return "\n\n".join(parts).strip()


def strip_markers(text: str) -> str:
    """Strip markers: used when counting characters to judge a text layer, and when returning content to a reader."""
    kept = [line for line in text.splitlines() if not PAGE_MARKER.match(line.strip())]
    return "\n".join(kept).strip()
