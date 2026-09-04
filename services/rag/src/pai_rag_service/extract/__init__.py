"""File -> text, under one invariant: a broken file may only break itself, so every path
returns :class:`Extracted` or raises with the path attached. PDFs cascade from text layer
to per-page VLM OCR below a character threshold; format comes from the extension."""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from pathlib import Path

from pai_rag_service.config import OcrConfig, ProviderConfig
from pai_rag_service.errors import ExtractError, UnsupportedFile
from pai_rag_service.extract.ocr import PROMPT, ocr_image_bytes, read_pdf_pages
from pai_rag_service.extract.pages import split_pages, strip_markers

__all__ = [
    "EXTRACT_VERSION",
    "Extracted",
    "MAX_FILE_BYTES",
    "SUPPORTED_EXTENSIONS",
    "extract",
    "format_for",
]

log = logging.getLogger(__name__)

#: Extractor version. Indexing is incremental, so bump this whenever the extractor produces different output for the same file; the next open invalidates old fingerprints.
EXTRACT_VERSION = 1

#: 64 MiB - where markitdown and the page renderer still fit in RAM; both read the whole file into memory.
MAX_FILE_BYTES = 64 * 1024 * 1024

#: How many leading bytes are inspected to call a file binary.
BINARY_PROBE = 4096

TEXT_EXTENSIONS = frozenset(
    {".txt", ".text", ".log", ".rst", ".adoc", ".org", ".md", ".markdown", ".mdx"}
)
#: Formats markitdown reads better than anything we could write.
OFFICE_EXTENSIONS = frozenset({".docx", ".xlsx", ".xlsm", ".pptx", ".doc", ".xls", ".ppt"})
DATA_EXTENSIONS = frozenset({".csv", ".tsv", ".json", ".xml", ".yaml", ".yml"})
WEB_EXTENSIONS = frozenset({".html", ".htm", ".xhtml"})
IMAGE_EXTENSIONS = frozenset(
    {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff"}
)
PDF_EXTENSIONS = frozenset({".pdf"})
#: Source files really do turn up in a document library; read them as plain text.
CODE_EXTENSIONS = frozenset(
    {
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".kt", ".c", ".h",
        ".cpp", ".hpp", ".cs", ".rb", ".php", ".swift", ".sh", ".ps1", ".sql", ".toml",
        ".ini", ".cfg", ".conf", ".gradle", ".lua", ".r", ".m", ".scala", ".dart",
    }
)

SUPPORTED_EXTENSIONS = (
    TEXT_EXTENSIONS
    | OFFICE_EXTENSIONS
    | DATA_EXTENSIONS
    | WEB_EXTENSIONS
    | IMAGE_EXTENSIONS
    | PDF_EXTENSIONS
    | CODE_EXTENSIONS
)


def format_for(path: Path) -> str:
    """Format label, or `""` when the file is outside the readable set."""
    suffix = path.suffix.lower()
    if suffix in PDF_EXTENSIONS:
        return "pdf"
    if suffix in OFFICE_EXTENSIONS:
        return "office"
    if suffix in IMAGE_EXTENSIONS:
        return "image"
    if suffix in WEB_EXTENSIONS:
        return "html"
    if suffix in DATA_EXTENSIONS:
        return "data"
    if suffix in {".md", ".markdown", ".mdx"}:
        return "markdown"
    if suffix in CODE_EXTENSIONS:
        return "code"
    if suffix in TEXT_EXTENSIONS or suffix == "":
        return "text"
    return ""


@dataclass(slots=True)
class Extracted:
    """A file's text, plus what has to be said about how it was read."""

    text: str
    format: str
    title: str
    #: Page count, when that means anything for this format.
    pages: int = 0
    #: Which pages went through the VLM; the UI says "12/40 pages via OCR", which explains a slow ingest.
    ocr_pages: list[int] = field(default_factory=list)
    #: Pages skipped at the `OcrConfig.max_pages` cap; stated rather than swallowed, since a silently short library is the bug this package exists to fix.
    skipped_pages: int = 0

    @property
    def chars(self) -> int:
        return len(strip_markers(self.text))


def _looks_binary(data: bytes) -> bool:
    return b"\x00" in data[:BINARY_PROBE]


def _as_text(path: Path, data: bytes) -> str:
    """Bytes -> string, UTF-8 first then the Vietnamese Windows code pages; a mis-decoded file does not fail, it just never matches a query."""
    for codec in ("utf-8", "utf-8-sig", "cp1258", "cp1252"):
        try:
            return data.decode(codec)
        except UnicodeDecodeError:
            continue
    # No codec left to try; replacing bad characters beats dropping the file.
    return data.decode("utf-8", errors="replace")


def _markitdown_text(path: Path) -> str:
    """Run markitdown on a file. Synchronous and CPU-bound - the caller moves it to a thread."""
    from markitdown import MarkItDown

    converter = MarkItDown(enable_plugins=False)
    try:
        result = converter.convert(str(path))
    except Exception as err:  # markitdown raises whatever the underlying libraries raise
        raise ExtractError(str(path), f"markitdown không đọc được: {err}") from err
    return (result.text_content or "").strip()


def _pdf_text_layer(path: Path, data: bytes) -> list[str]:
    """The PDF text layer, per page. An empty page means that page has no text."""
    from io import BytesIO

    from pypdf import PdfReader

    try:
        reader = PdfReader(BytesIO(data))
        return [(page.extract_text() or "").strip() for page in reader.pages]
    except Exception as err:
        raise ExtractError(str(path), f"không mở được PDF: {err}") from err


async def _extract_pdf(
    path: Path,
    data: bytes,
    *,
    vision: ProviderConfig,
    ocr: OcrConfig,
) -> Extracted:
    native = await asyncio.to_thread(_pdf_text_layer, path, data)
    total = len(native)
    if total == 0:
        raise ExtractError(str(path), "PDF không có trang nào")

    dense = sum(len(page) for page in native)
    average = dense / total
    if average >= ocr.min_chars_per_page:
        # The text layer is dense enough. The fastest and most accurate path; leave the VLM alone.
        return Extracted(
            text=split_pages(native),
            format="pdf",
            title=path.stem,
            pages=total,
        )

    if not ocr.enabled:
        raise ExtractError(
            str(path),
            f"lớp chữ chỉ có {average:.0f} ký tự/trang và OCR đang tắt. Bật OCR trong "
            "Cài đặt để đọc tệp quét.",
        )
    if not vision.model.strip():
        raise ExtractError(
            str(path),
            "đây là tệp quét và chưa chọn mô hình vision. Chọn một mô hình đọc được ảnh "
            "trong Cài đặt rồi nạp lại.",
        )

    read, skipped = await read_pdf_pages(data, path=str(path), provider=vision, ocr=ocr)
    # Keep the original text layer on pages that were already dense: OCR on clean print only makes it worse, and vision models often drop tables.
    merged: list[str] = []
    ocr_pages: list[int] = []
    for index in range(total):
        original = native[index] if index < len(native) else ""
        scanned = read[index] if index < len(read) else ""
        if len(original) >= ocr.min_chars_per_page:
            merged.append(original)
        elif scanned:
            merged.append(scanned)
            ocr_pages.append(index + 1)
        else:
            merged.append(original)

    if not any(part.strip() for part in merged):
        raise ExtractError(
            str(path),
            f"mô hình {vision.model} đã chạy nhưng không đọc được chữ nào trong tệp này",
        )

    return Extracted(
        text=split_pages(merged),
        format="pdf",
        title=path.stem,
        pages=total,
        ocr_pages=ocr_pages,
        skipped_pages=skipped,
    )


async def _extract_image(
    path: Path, data: bytes, *, vision: ProviderConfig
) -> Extracted:
    import httpx

    if not vision.model.strip():
        raise UnsupportedFile(
            str(path),
            "đây là ảnh và chưa chọn mô hình vision. Chọn một mô hình đọc được ảnh "
            "trong Cài đặt rồi nạp lại.",
        )
    async with httpx.AsyncClient() as client:
        text = await ocr_image_bytes(client, data, provider=vision, prompt=PROMPT)
    if not text.strip():
        raise ExtractError(str(path), f"mô hình {vision.model} không đọc được chữ nào trong ảnh")
    return Extracted(text=text, format="image", title=path.stem, pages=1, ocr_pages=[1])


async def extract(
    path: Path,
    *,
    vision: ProviderConfig,
    ocr: OcrConfig,
) -> Extracted:
    """Read a file into text; raises :class:`UnsupportedFile` for formats outside the readable set (not worth retrying) and :class:`ExtractError` for everything else."""
    if not path.is_file():
        raise ExtractError(str(path), "không phải một tệp")

    size = path.stat().st_size
    if size == 0:
        raise ExtractError(str(path), "tệp rỗng")
    if size > MAX_FILE_BYTES:
        raise UnsupportedFile(
            str(path),
            f"tệp {size / 1024 / 1024:.0f} MB vượt trần {MAX_FILE_BYTES // 1024 // 1024} MB",
        )

    kind = format_for(path)
    if not kind:
        shown = path.suffix or "(không đuôi)"
        raise UnsupportedFile(str(path), f"chưa đọc được định dạng `{shown}`")

    data = await asyncio.to_thread(path.read_bytes)

    if kind == "pdf":
        return await _extract_pdf(path, data, vision=vision, ocr=ocr)
    if kind == "image":
        return await _extract_image(path, data, vision=vision)
    if kind in {"office", "html", "data"}:
        text = await asyncio.to_thread(_markitdown_text, path)
        if not text:
            raise ExtractError(str(path), "markitdown đọc ra một tài liệu rỗng")
        return Extracted(text=text, format=kind, title=path.stem)

    # Everything else is plain text: markdown, source, txt.
    if _looks_binary(data):
        raise UnsupportedFile(str(path), "trông như tệp nhị phân dù mang đuôi văn bản")
    text = _as_text(path, data).strip()
    if not text:
        raise ExtractError(str(path), "tệp chỉ có khoảng trắng")
    return Extracted(text=text, format=kind, title=path.stem)
