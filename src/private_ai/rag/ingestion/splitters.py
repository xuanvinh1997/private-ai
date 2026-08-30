"""The chunker: heading-aware, page-aware, and deliberately not recursive.

A Markdown heading opens a section, and every chunk carries the heading it fell under so
a citation can name where in the document it came from. A page marker left by a loader
sets the page number for everything after it. Both are flush points, which is why a
chunk never straddles a heading or a page break.

Sizes come from settings rather than from LangChain's defaults: 1400 characters was
picked against the embedding model actually in use, and shrinking it silently would
change every stored vector's meaning.
"""

from __future__ import annotations

import re
from typing import TYPE_CHECKING, Any

from langchain_core.documents import Document
from langchain_text_splitters import TextSplitter

from private_ai.rag.ingestion.loaders import PAGE_MARKER

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.config import Settings

__all__ = [
    "DEFAULT_CHUNK_OVERLAP",
    "DEFAULT_CHUNK_SIZE",
    "DEFAULT_SECTION_TITLE",
    "SectionAwareTextSplitter",
]

DEFAULT_CHUNK_SIZE = 1400
DEFAULT_CHUNK_OVERLAP = 180
DEFAULT_SECTION_TITLE = "Nội dung"
HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
# document_sections.title is a plain TEXT column but the UI renders it inline; a runaway
# heading would push everything else off the row.
MAX_SECTION_TITLE = 240


class SectionAwareTextSplitter(TextSplitter):
    """Split text into chunks that remember their section and page."""

    def __init__(
        self,
        *,
        chunk_size: int = DEFAULT_CHUNK_SIZE,
        chunk_overlap: int = DEFAULT_CHUNK_OVERLAP,
        **kwargs: Any,
    ) -> None:
        super().__init__(chunk_size=chunk_size, chunk_overlap=chunk_overlap, **kwargs)

    @classmethod
    def from_settings(cls, settings: Settings) -> SectionAwareTextSplitter:
        return cls(
            chunk_size=settings.retrieval_chunk_size,
            chunk_overlap=settings.retrieval_chunk_overlap,
        )

    def split_text(self, text: str) -> list[str]:
        return [document.page_content for document in self.split_marked_text(text)]

    def split_marked_text(
        self,
        text: str,
        *,
        metadata: dict[str, Any] | None = None,
        first_chunk_index: int = 0,
        first_section_index: int = 0,
    ) -> list[Document]:
        """Chunk one blob of marked-up text into metadata-carrying documents."""
        documents, _, _ = self._split(
            text,
            metadata or {},
            first_chunk_index,
            first_section_index,
        )
        return documents

    def split_documents(self, documents: list[Document]) -> list[Document]:
        """Chunk loader output, numbering chunks and sections across the whole batch.

        The base class routes this through ``create_documents``, which only knows how to
        copy metadata forward; here each input needs its own section walk.
        """
        chunks: list[Document] = []
        chunk_index = 0
        section_index = 0
        for document in documents:
            produced, chunk_index, section_index = self._split(
                document.page_content,
                document.metadata,
                chunk_index,
                section_index,
            )
            chunks.extend(produced)
        return chunks

    def _split(
        self,
        text: str,
        metadata: dict[str, Any],
        first_chunk_index: int,
        first_section_index: int,
    ) -> tuple[list[Document], int, int]:
        documents: list[Document] = []
        chunk_index = first_chunk_index
        section_index = first_section_index
        section_title = DEFAULT_SECTION_TITLE
        section_level = 0
        page_number: int | None = None
        buffer: list[str] = []

        def flush() -> None:
            nonlocal chunk_index
            content = "\n".join(buffer).strip()
            buffer.clear()
            for chunk in self._split_block(content, self._chunk_size, self._chunk_overlap):
                documents.append(
                    Document(
                        page_content=chunk,
                        metadata={
                            **metadata,
                            "section_index": section_index,
                            "section_title": section_title,
                            "section_level": section_level,
                            "page_number": page_number,
                            "chunk_index": chunk_index,
                        },
                    )
                )
                chunk_index += 1

        for raw_line in text.splitlines():
            line = raw_line.rstrip()
            marker = PAGE_MARKER.fullmatch(line.strip())
            if marker:
                flush()
                page_number = int(marker.group(1))
                continue
            heading = HEADING.match(line)
            if heading:
                flush()
                # The very first heading of a document opens section 0 rather than
                # leaving an empty "Nội dung" section in front of it.
                if documents or section_title != DEFAULT_SECTION_TITLE:
                    section_index += 1
                section_title = heading.group(2).strip()[:MAX_SECTION_TITLE]
                section_level = len(heading.group(1))
                buffer.append(line)
                continue
            buffer.append(line)
        flush()
        return documents, chunk_index, section_index + 1

    @staticmethod
    def _split_block(text: str, size: int, overlap: int) -> list[str]:
        """Cut one section into overlapping windows on a sentence or line boundary.

        The boundary search only accepts a break past the midpoint: closer to the start it
        would produce a stub chunk that costs an embedding and says nothing.
        """
        normalized = re.sub(r"[ \t]+", " ", text).strip()
        if not normalized:
            return []
        chunks: list[str] = []
        start = 0
        while start < len(normalized):
            end = min(start + size, len(normalized))
            if end < len(normalized):
                boundary = max(
                    normalized.rfind("\n", start, end),
                    normalized.rfind(". ", start, end),
                )
                if boundary > start + size // 2:
                    end = boundary + 1
            chunk = normalized[start:end].strip()
            if chunk:
                chunks.append(chunk)
            if end >= len(normalized):
                break
            start = max(start + 1, end - overlap)
        return chunks
