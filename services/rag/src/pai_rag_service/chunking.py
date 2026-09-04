"""Split documents into chunks that remember their section and page. Three rules: a chunk
never crosses a heading or a page boundary, every chunk carries its section heading, and
no text is lost. Split priority: heading -> paragraph -> sentence -> hard cut."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

from langchain_core.documents import Document
from langchain_text_splitters import TextSplitter

from pai_rag_service.extract.pages import PAGE_MARKER

__all__ = [
    "DEFAULT_SECTION",
    "Chunk",
    "SectionAwareSplitter",
    "embedding_text",
]

#: Section assigned to text preceding the document's first heading.
DEFAULT_SECTION = "Nội dung"

HEADING = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
#: An unusually long heading breaks both the document table row and the citation line.
MAX_SECTION_TITLE = 240
#: Sentence end *followed by whitespace*; the second condition spares decimals and domain names.
SENTENCE_END = re.compile(r"(?<=[.!?…;])\s+")


@dataclass(slots=True)
class Chunk:
    """One chunk, enough to build a verifiable citation."""

    ordinal: int
    text: str
    section: str
    #: `0` means this format has no notion of pages.
    page: int = 0

    def to_document(self, **metadata: Any) -> Document:
        return Document(
            page_content=self.text,
            metadata={
                "ordinal": self.ordinal,
                "section": self.section,
                "page": self.page,
                **metadata,
            },
        )


def embedding_text_for(section: str, text: str) -> str:
    """The text actually embedded, with the section heading prepended so the semantic half sees the context the keyword half weights most; changing this changes every stored vector."""
    if section and section != DEFAULT_SECTION:
        return f"{section}\n\n{text}"
    return text


def embedding_text(chunk: Chunk) -> str:
    """:func:`embedding_text_for` for a :class:`Chunk`."""
    return embedding_text_for(chunk.section, chunk.text)


@dataclass(slots=True)
class _Unit:
    """The smallest unit the algorithm will separate."""

    text: str
    section: str
    page: int
    #: Does this unit force a new chunk (a heading, or a page turn)?
    flush: bool


class SectionAwareSplitter(TextSplitter):
    """Splits text into chunks that remember their section and page."""

    def __init__(
        self,
        *,
        chunk_size: int = 1400,
        chunk_overlap: int = 180,
        default_section: str = DEFAULT_SECTION,
    ) -> None:
        # An overlap at or above the chunk size would make each chunk contain the previous one and stop progress.
        size = max(1, chunk_size)
        super().__init__(chunk_size=size, chunk_overlap=min(max(0, chunk_overlap), size - 1))
        self.default_section = default_section

    # -- LangChain interface -----------------------------------------------------------

    def split_text(self, text: str) -> list[str]:
        return [chunk.text for chunk in self.split(text)]

    # -- the real work -----------------------------------------------------------------

    def split(self, text: str) -> list[Chunk]:
        """Split, returning chunks with their section and page."""
        units = self._units(text)
        return self._pack(units)

    def _units(self, text: str) -> list[_Unit]:
        """Step one: text -> units, in the priority order named at the top of the file."""
        section = self.default_section
        page = 0
        units: list[_Unit] = []
        # The page just turned, so the next content unit must open a chunk; a flag, because a marker is not content and must not own a unit.
        page_turned = False

        for block in self._blocks(text):
            marker = PAGE_MARKER.match(block.strip())
            if marker:
                page = int(marker.group(1))
                page_turned = True
                continue

            heading = HEADING.match(block.strip())
            if heading:
                section = heading.group(2).strip()[:MAX_SECTION_TITLE] or self.default_section
                # A heading line *is* content: it carries text a query may match, and it opens the section it names.
                units.append(_Unit(block.strip(), section, page, flush=True))
                page_turned = False
                continue

            for piece in self._fit(block.strip()):
                units.append(_Unit(piece, section, page, flush=page_turned))
                page_turned = False
        return units

    @staticmethod
    def _blocks(text: str) -> list[str]:
        """A block is a marker line, a heading line, or a run of adjacent lines between blank lines."""
        blocks: list[str] = []
        open_lines: list[str] = []

        def close() -> None:
            if open_lines:
                blocks.append("\n".join(open_lines))
                open_lines.clear()

        for line in text.splitlines():
            stripped = line.strip()
            if not stripped:
                close()
                continue
            if PAGE_MARKER.match(stripped) or HEADING.match(stripped):
                close()
                blocks.append(stripped)
                continue
            open_lines.append(line.rstrip())
        close()
        return blocks

    def _fit(self, block: str) -> list[str]:
        """A block longer than a chunk drops to sentences, then to a hard cut."""
        if len(block) <= self._chunk_size:
            return [block]

        out: list[str] = []
        for sentence in (part.strip() for part in SENTENCE_END.split(block)):
            if not sentence:
                continue
            if len(sentence) <= self._chunk_size:
                out.append(sentence)
                continue
            # A "sentence" longer than a chunk is a table or a code block; slide back to a word boundary anyway, since cutting mid-word hurts both embedding and reading.
            out.extend(self._hard_split(sentence))
        return out

    def _hard_split(self, text: str) -> list[str]:
        out: list[str] = []
        start = 0
        limit = self._chunk_size
        while start < len(text):
            end = min(start + limit, len(text))
            if end < len(text):
                space = text.rfind(" ", start + limit * 3 // 4, end)
                if space > start:
                    end = space
            piece = text[start:end].strip()
            if piece:
                out.append(piece)
            # `end == start` only happens on an all-whitespace slice; step forward so the loop always advances.
            start = end if end > start else start + limit
        return out

    def _pack(self, units: list[_Unit]) -> list[Chunk]:
        """Step two: pack units into chunks, then look back for the overlap."""
        chunks: list[Chunk] = []
        open_units: list[_Unit] = []
        carry = ""

        # Threshold before a heading may open a new chunk; without it a document of short headings yields one chunk per heading.
        min_fill = self._chunk_size // 3

        def flush() -> None:
            nonlocal carry, open_units
            if not open_units:
                return
            body = "\n\n".join(unit.text for unit in open_units)
            text = f"{carry}\n\n{body}".strip() if carry else body
            chunks.append(
                Chunk(
                    ordinal=len(chunks),
                    text=text,
                    # Section and page come from the *first* unit, not the carried-over overlap.
                    section=open_units[0].section,
                    page=open_units[0].page,
                )
            )
            carry = self._overlap_tail(text)
            open_units = []

        filled = 0
        for unit in units:
            too_full = filled + len(unit.text) > self._chunk_size
            new_section = unit.flush and filled >= min_fill
            if open_units and (too_full or new_section):
                flush()
                filled = len(carry)
            open_units.append(unit)
            filled += len(unit.text)
        flush()
        return chunks

    def _overlap_tail(self, text: str) -> str:
        """Tail of the chunk just closed, for the next one to overlap; cut by character then slid to the next word, because unit-sized overlap silently produced none."""
        if self._chunk_overlap <= 0 or len(text) <= self._chunk_overlap:
            return ""
        tail = text[-self._chunk_overlap :]
        space = tail.find(" ")
        # A tail with no space is a word longer than the overlap; keeping it beats dropping the overlap entirely.
        return tail[space + 1 :].strip() if space != -1 else tail.strip()
