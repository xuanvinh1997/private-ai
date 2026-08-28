from __future__ import annotations

import asyncio
import re
import shutil
import threading
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from uuid import uuid4
from xml.etree import ElementTree
from zipfile import BadZipFile, ZipFile

from markitdown import MarkItDown
from openai import OpenAI
from pypdf import PdfReader

from private_ai_api.database import Database
from private_ai_api.services.lightrag_store import LightRagStore
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRouter

TEXT_EXTENSIONS = {".txt", ".md", ".markdown", ".csv", ".json", ".yaml", ".yml"}
OFFICE_EXTENSIONS = {".docx", ".pptx", ".xlsx"}
IMAGE_EXTENSIONS = {".bmp", ".gif", ".jpeg", ".jpg", ".png", ".tif", ".tiff", ".webp"}
PAGE_MARKER = re.compile(r"^<!--\s*private-ai-page:(\d+)\s*-->$")
MARKITDOWN_PAGE_HEADING = re.compile(r"^#{1,6}\s+Page\s+\d+\s*$", re.IGNORECASE)
IMAGE_OCR_PROMPT = (
    "Extract every visible word from this image. Preserve headings, lists and tables as "
    "Markdown. Do not summarize, translate or invent missing text."
)


OCR_ENABLED_KEY = "ocr_enabled"


class UnsupportedDocument(RuntimeError):
    pass


class DocumentProcessor:
    def __init__(
        self,
        database: Database,
        lightrag: LightRagStore,
        *,
        ollama_url: str = "http://127.0.0.1:11434",
        vision_model: str = "",
        ai: ProviderRouter | None = None,
        resolve_vision_endpoint: Callable[[], tuple[str, str]] | None = None,
    ) -> None:
        self.database = database
        self.lightrag = lightrag
        self.ai = ai
        self.ollama_url = ollama_url.rstrip("/")
        # markitdown-ocr reads images through an OpenAI-shaped client, which has to point at
        # whichever provider is selected rather than always at the local Ollama.
        self.resolve_vision_endpoint = resolve_vision_endpoint or (
            lambda: (f"{ollama_url.rstrip('/')}/v1", "ollama")
        )
        self.vision_model = vision_model.strip()
        self._markitdown: MarkItDown | None = None
        self._markitdown_signature: tuple[str, bool] | None = None
        self._markitdown_lock = threading.RLock()
        self._locks: dict[str, asyncio.Lock] = {}

    async def process_pending(self) -> None:
        """Finish anything the last run left behind, then index whatever is not in the graph."""
        pending = self.database.fetch_all(
            """
            SELECT id FROM documents
            WHERE status IN ('queued', 'processing')
            ORDER BY created_at
            """
        )
        for document in pending:
            await self.process(str(document["id"]))
        unindexed = self.database.fetch_all(
            """
            SELECT id FROM documents
            WHERE status = 'ready' AND extracted_text IS NOT NULL AND indexed_at IS NULL
            ORDER BY created_at
            """
        )
        for document in unindexed:
            await self.index_document(str(document["id"]))

    async def process(self, document_id: str) -> None:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            vision_model = await self.resolve_vision_model()
            await asyncio.to_thread(self._process_sync, document_id, vision_model)
        await self.index_document(document_id)

    async def resolve_vision_model(self) -> str:
        """The model OCR reads with: the explicit pick, else any vision model on offer.

        Ticking OCR is the whole instruction, so a provider that already serves a
        vision-capable model should not need a second, separate choice.
        """
        stored = self.database.fetch_one(
            "SELECT model_name FROM model_defaults WHERE task = 'vision'"
        )
        if stored and str(stored["model_name"]).strip():
            return str(stored["model_name"]).strip()
        if self.vision_model:
            return self.vision_model
        if self.ai is None:
            return ""
        try:
            models = await self.ai.list_models()
        except ProviderUnavailable:
            return ""
        return next((model.name for model in models if "vision" in model.capabilities), "")

    async def index_document(self, document_id: str) -> bool:
        """Hand the extracted text to LightRAG, which chunks, embeds and builds the graph."""
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            document = self.database.fetch_one(
                "SELECT workspace_id, filename, extracted_text, status "
                "FROM documents WHERE id = ?",
                (document_id,),
            )
            if not document or document["status"] != "ready":
                return False
            indexed = await self.lightrag.index_document(
                str(document["workspace_id"]),
                document_id,
                str(document["filename"]),
                str(document["extracted_text"] or ""),
            )
            self.database.execute(
                "UPDATE documents SET indexed_at = ? WHERE id = ?",
                (datetime.now(UTC).isoformat() if indexed else None, document_id),
            )
            return indexed

    async def delete(self, document_id: str) -> bool:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            document = self.database.fetch_one(
                "SELECT workspace_id FROM documents WHERE id = ?",
                (document_id,),
            )
            if document:
                await self.lightrag.delete_document(str(document["workspace_id"]), document_id)
            return await asyncio.to_thread(self._delete_sync, document_id)

    async def search(
        self,
        query: str,
        limit: int = 5,
        *,
        workspace_id: str,
    ) -> list[dict[str, object]]:
        """Search one workspace. Never returns another workspace's content."""
        return await self.lightrag.search(query, workspace_id, limit)





    def _delete_sync(self, document_id: str) -> bool:
        document = self.database.fetch_one(
            "SELECT source_path FROM documents WHERE id = ?",
            (document_id,),
        )
        if not document:
            return False
        source_path = Path(document["source_path"])
        self.database.execute("DELETE FROM documents WHERE id = ?", (document_id,))
        shutil.rmtree(source_path.parent, ignore_errors=True)
        return True













    def _process_sync(self, document_id: str, vision_model: str = "") -> None:
        document = self.database.fetch_one("SELECT * FROM documents WHERE id = ?", (document_id,))
        if not document:
            return
        job_id = str(uuid4())
        created_at = datetime.now(UTC).isoformat()
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "document_ingestion",
                "status": "processing",
                "progress": 0.1,
                "payload": {"document_id": document_id},
                "created_at": created_at,
                "updated_at": created_at,
            }
        )
        self._update_document(document_id, status="processing", error=None)
        try:
            source_path = Path(document["source_path"])
            ocr_allowed = self.ocr_enabled(document_id)
            text = self._extract(source_path, ocr_allowed, vision_model)
            normalized = "\n".join(line.rstrip() for line in text.splitlines()).strip()
            meaningful = "\n".join(
                line
                for line in normalized.splitlines()
                if not PAGE_MARKER.fullmatch(line.strip())
            ).strip()
            if meaningful:
                self._update_document(
                    document_id,
                    status="ready",
                    extracted_text=normalized,
                    error=None,
                )
                job_status = "completed"
                progress = 1.0
                error = None
            else:
                error = self._ocr_gap(ocr_allowed, vision_model)
                self._update_document(
                    document_id,
                    status="needs_ocr",
                    extracted_text=None,
                    error=error,
                )
                job_status = "needs_ocr"
                progress = 1.0
        except Exception as exc:
            self._update_document(
                document_id,
                status="failed",
                extracted_text=None,
                error=str(exc),
            )
            job_status = "failed"
            progress = 1.0
            error = str(exc)
        updated_at = datetime.now(UTC).isoformat()
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "document_ingestion",
                "status": job_status,
                "progress": progress,
                "payload": {"document_id": document_id},
                "error": error,
                "created_at": created_at,
                "updated_at": updated_at,
            }
        )





    def _update_document(
        self,
        document_id: str,
        *,
        status: str,
        extracted_text: str | None = None,
        error: str | None,
    ) -> None:
        self.database.execute(
            """
            UPDATE documents
            SET status = ?, extracted_text = ?, error = ?, updated_at = ?
            WHERE id = ?
            """,
            (status, extracted_text, error, datetime.now(UTC).isoformat(), document_id),
        )

    @staticmethod
    def _ocr_gap(ocr_allowed: bool, vision_model: str) -> str:
        """Why a file produced no text, in the terms the user can act on."""
        if not ocr_allowed:
            return "OCR is off for this file, and it has no readable text layer"
        if not vision_model:
            return (
                "Nhà cung cấp đang bật không có mô hình nào đọc được ảnh. Cài hoặc chọn một "
                "mô hình vision, rồi bấm đọc lại."
            )
        return f"Mô hình {vision_model} đã chạy nhưng không đọc được chữ nào trong tệp này"

    def ocr_enabled(self, document_id: str | None = None) -> bool:
        """Whether reading may fall back to OCR: the document's own choice, else the default."""
        if document_id is not None:
            row = self.database.fetch_one(
                "SELECT use_ocr FROM documents WHERE id = ?",
                (document_id,),
            )
            if row is not None and row["use_ocr"] is not None:
                return bool(row["use_ocr"])
        stored = self.database.fetch_one(
            "SELECT value FROM app_state WHERE key = ?",
            (OCR_ENABLED_KEY,),
        )
        return stored is None or str(stored["value"]) == "1"

    def _markitdown_converter(self, ocr: bool, vision_model: str) -> MarkItDown:
        model = vision_model if ocr else ""
        signature = (model, ocr)
        with self._markitdown_lock:
            if self._markitdown is not None and self._markitdown_signature == signature:
                return self._markitdown
            # The plugin set is where markitdown-ocr lives, so turning OCR off has to drop it
            # along with the vision model.
            options: dict[str, object] = {"enable_plugins": ocr}
            if model:
                options.update(
                    {
                        "llm_client": self._vision_client(),
                        "llm_model": model,
                        "llm_prompt": IMAGE_OCR_PROMPT,
                    }
                )
            self._markitdown = MarkItDown(**options)
            self._markitdown_signature = signature
            return self._markitdown

    def _vision_client(self) -> OpenAI:
        base_url, api_key = self.resolve_vision_endpoint()
        return OpenAI(base_url=base_url, api_key=api_key or "unused")

    def _extract_markitdown(self, path: Path, ocr: bool, vision_model: str = "") -> str:
        with self._markitdown_lock:
            converter = self._markitdown_converter(ocr, vision_model)
            return converter.convert_local(path).markdown.strip()

    @staticmethod
    def _native_pdf_text(path: Path) -> str:
        reader = PdfReader(path)
        return "\n\n".join(
            f"<!-- private-ai-page:{index} -->\n{page.extract_text() or ''}"
            for index, page in enumerate(reader.pages, start=1)
        )

    def _extract(self, path: Path, ocr: bool, vision_model: str = "") -> str:
        extension = path.suffix.lower()
        if extension in TEXT_EXTENSIONS:
            return path.read_text(encoding="utf-8", errors="replace")
        if extension == ".pdf":
            native = self._native_pdf_text(path)
            native_text = "\n".join(
                line for line in native.splitlines() if not PAGE_MARKER.fullmatch(line.strip())
            ).strip()
            try:
                converted = self._extract_markitdown(path, ocr, vision_model)
            except Exception as exc:
                if native_text:
                    return native
                raise RuntimeError(f"OCR không thể xử lý {path.name}: {exc}") from exc
            converted_text = "\n".join(
                line
                for line in converted.splitlines()
                if not MARKITDOWN_PAGE_HEADING.fullmatch(line.strip())
            ).strip()
            if "*[Image OCR]" in converted or (not native_text and converted_text):
                return converted
            return native
        if extension in OFFICE_EXTENSIONS:
            try:
                converted = self._extract_markitdown(path, ocr, vision_model)
            except Exception:
                converted = ""
            return converted or self._extract_office_xml(path, extension)
        if extension in IMAGE_EXTENSIONS:
            try:
                converted = self._extract_markitdown(path, ocr, vision_model)
            except Exception as exc:
                raise RuntimeError(f"OCR không thể xử lý {path.name}: {exc}") from exc
            if "# Description:" in converted or "*[Image OCR]" in converted:
                return f"<!-- private-ai-page:1 -->\n# Image OCR\n\n{converted}"
            return ""
        raise UnsupportedDocument(f"Unsupported document type: {extension or 'unknown'}")

    @staticmethod
    def _extract_office_xml(path: Path, extension: str) -> str:
        prefixes = {
            ".docx": ("word/document.xml",),
            ".pptx": ("ppt/slides/slide",),
            ".xlsx": ("xl/sharedStrings.xml", "xl/worksheets/sheet"),
        }[extension]
        fragments: list[str] = []
        try:
            with ZipFile(path) as archive:
                names = sorted(
                    name
                    for name in archive.namelist()
                    if name.endswith(".xml") and name.startswith(prefixes)
                )
                for name in names:
                    root = ElementTree.fromstring(archive.read(name))
                    text = " ".join(value.strip() for value in root.itertext() if value.strip())
                    if text:
                        fragments.append(text)
        except (BadZipFile, ElementTree.ParseError) as exc:
            raise UnsupportedDocument(f"Cannot read {extension} document") from exc
        return "\n\n".join(fragments)
