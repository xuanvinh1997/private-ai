"""One LangChain loader per input family.

Every loader yields a single ``Document`` holding the whole file, because the
downstream splitter is the thing that knows about sections and pages. The page markers
these loaders write (``<!-- private-ai-page:N -->``) are load-bearing: the splitter
turns them into ``page_number`` metadata and the citation UI renders that number, so a
loader that drops them silently costs every citation its page.

Parsing is CPU-bound and holds the GIL, so each loader does its work on a worker thread
rather than on the caller's event loop.
"""

from __future__ import annotations

import asyncio
import re
from collections.abc import AsyncIterator
from pathlib import Path
from xml.etree import ElementTree
from zipfile import BadZipFile, ZipFile

from langchain_core.document_loaders import BaseLoader
from langchain_core.documents import Document
from pypdf import PdfReader

from private_ai.rag.ingestion.ocr import OCR_MARKER, MarkItDownConverter, VisionOcrLoader

__all__ = [
    "IMAGE_EXTENSIONS",
    "MARKITDOWN_PAGE_HEADING",
    "OFFICE_EXTENSIONS",
    "PAGE_MARKER",
    "TEXT_EXTENSIONS",
    "ImageLoader",
    "OfficeLoader",
    "PdfLoader",
    "TextFileLoader",
    "UnsupportedDocument",
    "loader_for",
    "page_marker",
    "strip_page_markers",
]

TEXT_EXTENSIONS = frozenset({".txt", ".md", ".markdown", ".csv", ".json", ".yaml", ".yml"})
OFFICE_EXTENSIONS = frozenset({".docx", ".pptx", ".xlsx"})
IMAGE_EXTENSIONS = frozenset({".bmp", ".gif", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"})

PAGE_MARKER = re.compile(r"^<!--\s*private-ai-page:(\d+)\s*-->$")
# MarkItDown writes its own "# Page 3" headings. They are page *labels*, not content, and
# counting them as text would make a scan of blank pages look like a readable document.
MARKITDOWN_PAGE_HEADING = re.compile(r"^#{1,6}\s+Page\s+\d+\s*$", re.IGNORECASE)

OFFICE_XML_PREFIXES: dict[str, tuple[str, ...]] = {
    ".docx": ("word/document.xml",),
    ".pptx": ("ppt/slides/slide",),
    ".xlsx": ("xl/sharedStrings.xml", "xl/worksheets/sheet"),
}


class UnsupportedDocument(RuntimeError):
    """The file's extension is not one we know how to read."""


def page_marker(number: int) -> str:
    return f"<!-- private-ai-page:{number} -->"


def strip_page_markers(text: str) -> str:
    """The text as a reader would see it, so emptiness can be judged on content alone."""
    return "\n".join(
        line for line in text.splitlines() if not PAGE_MARKER.fullmatch(line.strip())
    ).strip()


class TextFileLoader(BaseLoader):
    """Plain text, Markdown and the structured-text formats we store verbatim."""

    def __init__(self, path: str | Path) -> None:
        self.path = Path(path)

    async def alazy_load(self) -> AsyncIterator[Document]:
        # An upload may be as large as the upload cap allows, so the read stays off the loop.
        text = await asyncio.to_thread(self.path.read_text, encoding="utf-8", errors="replace")
        yield Document(
            page_content=text,
            metadata={"source": str(self.path), "loader": "text"},
        )


class PdfLoader(BaseLoader):
    """PDF, read twice and judged once.

    ``pypdf`` gives the native text layer with a marker per page; MarkItDown (with the OCR
    plugin) gives what a vision model can see. The converted text wins when OCR actually
    ran, or when the native layer is empty — a born-digital PDF otherwise loses its exact
    text to a model's re-transcription of it.
    """

    def __init__(
        self,
        path: str | Path,
        *,
        converter: MarkItDownConverter | None = None,
        ocr: bool = True,
        vision_model: str = "",
    ) -> None:
        self.path = Path(path)
        self.converter = converter or MarkItDownConverter()
        self.ocr = ocr
        self.vision_model = vision_model

    def _native_text(self) -> str:
        reader = PdfReader(self.path)
        return "\n\n".join(
            f"{page_marker(index)}\n{page.extract_text() or ''}"
            for index, page in enumerate(reader.pages, start=1)
        )

    async def alazy_load(self) -> AsyncIterator[Document]:
        native = await asyncio.to_thread(self._native_text)
        native_text = strip_page_markers(native)
        try:
            converted = await self.converter.aconvert(self.path, self.ocr, self.vision_model)
        except Exception as exc:
            if native_text:
                yield self._document(native, ocr=False)
                return
            raise RuntimeError(f"OCR không thể xử lý {self.path.name}: {exc}") from exc
        converted_text = "\n".join(
            line
            for line in converted.splitlines()
            if not MARKITDOWN_PAGE_HEADING.fullmatch(line.strip())
        ).strip()
        if OCR_MARKER in converted or (not native_text and converted_text):
            yield self._document(converted, ocr=True)
            return
        yield self._document(native, ocr=False)

    def _document(self, text: str, *, ocr: bool) -> Document:
        return Document(
            page_content=text,
            metadata={"source": str(self.path), "loader": "pdf", "ocr": ocr},
        )


class OfficeLoader(BaseLoader):
    """docx / pptx / xlsx through MarkItDown, with a raw-ZIP reading as the safety net."""

    def __init__(
        self,
        path: str | Path,
        *,
        converter: MarkItDownConverter | None = None,
        ocr: bool = True,
        vision_model: str = "",
    ) -> None:
        self.path = Path(path)
        self.converter = converter or MarkItDownConverter()
        self.ocr = ocr
        self.vision_model = vision_model

    async def alazy_load(self) -> AsyncIterator[Document]:
        try:
            converted = await self.converter.aconvert(self.path, self.ocr, self.vision_model)
        except Exception:
            converted = ""
        text = converted or await asyncio.to_thread(
            extract_office_xml, self.path, self.path.suffix.lower()
        )
        yield Document(
            page_content=text,
            metadata={"source": str(self.path), "loader": "office"},
        )


class ImageLoader(VisionOcrLoader):
    """An image is only ever text if the vision model read text out of it."""


def extract_office_xml(path: Path, extension: str) -> str:
    """Pull the visible strings straight out of the OOXML parts.

    Worth keeping even though MarkItDown normally wins: a file it refuses outright still
    has readable XML inside, and returning that beats telling the user the document is
    empty.
    """
    prefixes = OFFICE_XML_PREFIXES.get(extension)
    if prefixes is None:
        raise UnsupportedDocument(f"Cannot read {extension} document")
    fragments: list[str] = []
    try:
        with ZipFile(path) as archive:
            names = sorted(
                name
                for name in archive.namelist()
                if name.endswith(".xml") and name.startswith(prefixes)
            )
            for name in names:
                root = ElementTree.fromstring(archive.read(name))  # noqa: S314
                text = " ".join(value.strip() for value in root.itertext() if value.strip())
                if text:
                    fragments.append(text)
    except (BadZipFile, ElementTree.ParseError) as exc:
        raise UnsupportedDocument(f"Cannot read {extension} document") from exc
    return "\n\n".join(fragments)


def loader_for(
    path: str | Path,
    *,
    ocr: bool,
    vision_model: str,
    converter: MarkItDownConverter | None = None,
) -> BaseLoader:
    """Pick the loader for one file, by extension."""
    target = Path(path)
    extension = target.suffix.lower()
    if extension in TEXT_EXTENSIONS:
        return TextFileLoader(target)
    if extension == ".pdf":
        return PdfLoader(target, converter=converter, ocr=ocr, vision_model=vision_model)
    if extension in OFFICE_EXTENSIONS:
        return OfficeLoader(target, converter=converter, ocr=ocr, vision_model=vision_model)
    if extension in IMAGE_EXTENSIONS:
        return ImageLoader(target, converter=converter, ocr=ocr, vision_model=vision_model)
    raise UnsupportedDocument(f"Unsupported document type: {extension or 'unknown'}")
