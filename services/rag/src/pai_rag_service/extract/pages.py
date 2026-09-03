"""Đánh dấu ranh giới trang, để một trích dẫn nói được nó nằm ở trang mấy.

Marker là một comment HTML vì nó phải sống sót qua Markdown mà không hiện ra: markitdown
trả về Markdown, người dùng có thể đọc chính đoạn đó trong giao diện, và một dòng
``--- trang 7 ---`` giữa câu văn là rác nhìn thấy được. Comment thì trình dựng Markdown
nào cũng nuốt.

Marker là **load-bearing**: :mod:`pai_rag_service.chunking` biến nó thành metadata
``page`` và giao diện trích dẫn vẽ con số ấy ra. Một bộ rút chữ đánh rơi marker sẽ khiến
mọi trích dẫn của định dạng đó mất số trang, mà không có gì báo lỗi.
"""

from __future__ import annotations

import re

__all__ = ["PAGE_MARKER", "mark", "split_pages", "strip_markers"]

#: Khớp đúng một dòng chỉ chứa marker và không gì khác.
PAGE_MARKER = re.compile(r"^<!--\s*pai-page:(\d+)\s*-->$")


def mark(page: int) -> str:
    """Marker mở đầu một trang. Đếm từ 1 — đó là con số người dùng thấy trên tệp PDF."""
    return f"<!-- pai-page:{page} -->"


def split_pages(pages: list[str]) -> str:
    """Ghép các trang lại, mỗi trang mở đầu bằng marker của nó."""
    parts: list[str] = []
    for index, body in enumerate(pages, start=1):
        parts.append(mark(index))
        parts.append(body.strip())
    return "\n\n".join(parts).strip()


def strip_markers(text: str) -> str:
    """Bỏ marker khỏi văn bản.

    Dùng ở đúng hai chỗ: lúc đếm ký tự để quyết định lớp chữ có đáng tin không — đếm cả
    marker sẽ khiến một tệp quét toàn trang trống trông như có chữ — và lúc trả nội dung
    ra cho người đọc.
    """
    kept = [line for line in text.splitlines() if not PAGE_MARKER.match(line.strip())]
    return "\n".join(kept).strip()
