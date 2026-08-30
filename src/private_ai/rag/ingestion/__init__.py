"""Turning files into searchable chunks.

The stages are separate on purpose: a loader knows how to read one file family, the
splitter knows how a document is shaped, and the pipeline owns the parts that are about
*running* the work — the cross-process claim, the progress ladder and the guarantee that
a document is only ever ready when every one of its chunks has a vector.
"""

from __future__ import annotations

from private_ai.rag.ingestion.loaders import (
    IMAGE_EXTENSIONS,
    OFFICE_EXTENSIONS,
    PAGE_MARKER,
    TEXT_EXTENSIONS,
    ImageLoader,
    OfficeLoader,
    PdfLoader,
    TextFileLoader,
    UnsupportedDocument,
    loader_for,
    page_marker,
    strip_page_markers,
)
from private_ai.rag.ingestion.ocr import (
    IMAGE_OCR_PROMPT,
    MarkItDownConverter,
    VisionOcrLoader,
    ocr_gap,
)
from private_ai.rag.ingestion.pipeline import IngestionPipeline
from private_ai.rag.ingestion.splitters import (
    DEFAULT_CHUNK_OVERLAP,
    DEFAULT_CHUNK_SIZE,
    SectionAwareTextSplitter,
)

__all__ = [
    "DEFAULT_CHUNK_OVERLAP",
    "DEFAULT_CHUNK_SIZE",
    "IMAGE_EXTENSIONS",
    "IMAGE_OCR_PROMPT",
    "OFFICE_EXTENSIONS",
    "PAGE_MARKER",
    "TEXT_EXTENSIONS",
    "ImageLoader",
    "IngestionPipeline",
    "MarkItDownConverter",
    "OfficeLoader",
    "PdfLoader",
    "SectionAwareTextSplitter",
    "TextFileLoader",
    "UnsupportedDocument",
    "VisionOcrLoader",
    "loader_for",
    "ocr_gap",
    "page_marker",
    "strip_page_markers",
]
