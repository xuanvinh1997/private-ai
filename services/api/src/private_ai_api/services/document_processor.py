from __future__ import annotations

import asyncio
import os
import re
import shutil
import subprocess
import tempfile
import threading
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


class OcrUnavailable(RuntimeError):
    pass


class DocumentProcessor:
    def __init__(
        self,
        database: Database,
        lightrag: LightRagStore,
        *,
        ollama_url: str = "http://127.0.0.1:11434",
        vision_model: str = "",
    ) -> None:
        self.database = database
        self.lightrag = lightrag
        self.ollama_url = ollama_url.rstrip("/")
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
            await asyncio.to_thread(self._process_sync, document_id)
        await self.index_document(document_id)

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













    def _process_sync(self, document_id: str) -> None:
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
            text = self._extract(source_path)
            normalized = "\n".join(line.rstrip() for line in text.splitlines()).strip()
            meaningful = "\n".join(
                line
                for line in normalized.splitlines()
                if not PAGE_MARKER.fullmatch(line.strip())
            ).strip()
            ocr_error: str | None = None
            extension = source_path.suffix.lower()
            ocr_allowed = self.ocr_enabled()
            if not meaningful and not ocr_allowed:
                ocr_error = "OCR is turned off, and this file has no readable text layer"
            if not meaningful and ocr_allowed and (
                extension == ".pdf" or extension in IMAGE_EXTENSIONS
            ):
                try:
                    normalized = (
                        self._ocr_pdf(source_path)
                        if extension == ".pdf"
                        else self._ocr_image(source_path)
                    )
                    meaningful = "\n".join(
                        line
                        for line in normalized.splitlines()
                        if not PAGE_MARKER.fullmatch(line.strip())
                    ).strip()
                except OcrUnavailable as exc:
                    ocr_error = str(exc)
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
                self._update_document(
                    document_id,
                    status="needs_ocr",
                    extracted_text=None,
                    error=ocr_error or "OCR completed but no text was detected",
                )
                job_status = "needs_ocr"
                progress = 1.0
                error = ocr_error or "OCR completed but no text was detected"
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

    def _active_vision_model(self) -> str:
        configured = self.database.fetch_one(
            "SELECT model_name FROM model_defaults WHERE task = 'vision'"
        )
        return str(configured["model_name"]).strip() if configured else self.vision_model

    def ocr_enabled(self) -> bool:
        """Whether reading a document may fall back to OCR. Defaults to on."""
        stored = self.database.fetch_one(
            "SELECT value FROM app_state WHERE key = ?",
            (OCR_ENABLED_KEY,),
        )
        return stored is None or str(stored["value"]) == "1"

    def _markitdown_converter(self) -> MarkItDown:
        ocr = self.ocr_enabled()
        model = self._active_vision_model() if ocr else ""
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
                        "llm_client": OpenAI(
                            base_url=f"{self.ollama_url}/v1",
                            api_key="ollama",
                        ),
                        "llm_model": model,
                        "llm_prompt": IMAGE_OCR_PROMPT,
                    }
                )
            self._markitdown = MarkItDown(**options)
            self._markitdown_signature = signature
            return self._markitdown

    def _extract_markitdown(self, path: Path) -> str:
        try:
            with self._markitdown_lock:
                return self._markitdown_converter().convert_local(path).markdown.strip()
        except Exception:
            return ""

    @staticmethod
    def _native_pdf_text(path: Path) -> str:
        reader = PdfReader(path)
        return "\n\n".join(
            f"<!-- private-ai-page:{index} -->\n{page.extract_text() or ''}"
            for index, page in enumerate(reader.pages, start=1)
        )

    def _extract(self, path: Path) -> str:
        extension = path.suffix.lower()
        if extension in TEXT_EXTENSIONS:
            return path.read_text(encoding="utf-8", errors="replace")
        if extension == ".pdf":
            native = self._native_pdf_text(path)
            converted = self._extract_markitdown(path)
            native_text = "\n".join(
                line for line in native.splitlines() if not PAGE_MARKER.fullmatch(line.strip())
            ).strip()
            converted_text = "\n".join(
                line
                for line in converted.splitlines()
                if not MARKITDOWN_PAGE_HEADING.fullmatch(line.strip())
            ).strip()
            if "*[Image OCR]" in converted or (not native_text and converted_text):
                return converted
            return native
        if extension in OFFICE_EXTENSIONS:
            return self._extract_markitdown(path) or self._extract_office_xml(path, extension)
        if extension in IMAGE_EXTENSIONS:
            converted = self._extract_markitdown(path)
            if "# Description:" in converted or "*[Image OCR]" in converted:
                return f"<!-- private-ai-page:1 -->\n# Image OCR\n\n{converted}"
            return ""
        raise UnsupportedDocument(f"Unsupported document type: {extension or 'unknown'}")

    @staticmethod
    def _tesseract_languages(tesseract: str) -> str:
        language_result = subprocess.run(  # noqa: S603
            [tesseract, "--list-langs"],
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
        available = {
            line.strip()
            for line in language_result.stdout.splitlines()
            if line.strip() and not line.startswith("List of available")
        }
        preferred = os.getenv("PRIVATE_AI_OCR_LANGUAGES", "vie+eng").split("+")
        selected = [language for language in preferred if language in available]
        if not selected and "eng" in available:
            selected = ["eng"]
        if not selected:
            raise OcrUnavailable("Tesseract has no configured OCR language data")
        return "+".join(selected)

    @staticmethod
    def _ocr_image(path: Path) -> str:
        tesseract = shutil.which("tesseract")
        if not tesseract:
            raise OcrUnavailable(
                "Image OCR requires Tesseract on PATH or PRIVATE_AI_VISION_MODEL"
            )
        recognized = subprocess.run(  # noqa: S603
            [
                tesseract,
                str(path),
                "stdout",
                "-l",
                DocumentProcessor._tesseract_languages(tesseract),
                "--psm",
                "6",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=180,
        )
        if recognized.returncode != 0:
            detail = recognized.stderr.strip()[-500:] or "unknown Tesseract error"
            raise OcrUnavailable(f"OCR failed for {path.name}: {detail}")
        text = recognized.stdout.strip()
        return f"<!-- private-ai-page:1 -->\n# Image OCR\n\n{text}" if text else ""

    @staticmethod
    def _ocr_pdf(path: Path) -> str:
        pdftoppm = shutil.which("pdftoppm")
        tesseract = shutil.which("tesseract")
        commands = (("pdftoppm", pdftoppm), ("tesseract", tesseract))
        missing = [name for name, value in commands if not value]
        if missing:
            raise OcrUnavailable(
                f"OCR requires {', '.join(missing)} on PATH (Poppler and Tesseract)"
            )

        with tempfile.TemporaryDirectory(prefix="private-ai-ocr-") as temp_dir:
            page_prefix = Path(temp_dir) / "page"
            converted = subprocess.run(  # noqa: S603
                [pdftoppm, "-png", "-r", "200", str(path), str(page_prefix)],
                check=False,
                capture_output=True,
                text=True,
                timeout=300,
            )
            if converted.returncode != 0:
                detail = converted.stderr.strip()[-500:] or "unknown Poppler error"
                raise OcrUnavailable(f"Cannot render PDF for OCR: {detail}")
            pages = sorted(Path(temp_dir).glob("page-*.png"))
            if not pages:
                raise OcrUnavailable("PDF renderer produced no pages")

            language = DocumentProcessor._tesseract_languages(tesseract)

            fragments: list[str] = []
            for page_number, page in enumerate(pages, start=1):
                recognized = subprocess.run(  # noqa: S603
                    [tesseract, str(page), "stdout", "-l", language, "--psm", "6"],
                    check=False,
                    capture_output=True,
                    text=True,
                    timeout=180,
                )
                if recognized.returncode != 0:
                    detail = recognized.stderr.strip()[-500:] or "unknown Tesseract error"
                    raise OcrUnavailable(f"OCR failed for {page.name}: {detail}")
                if recognized.stdout.strip():
                    fragments.append(
                        f"<!-- private-ai-page:{page_number} -->\n"
                        f"{recognized.stdout.strip()}"
                    )
            return "\n\n".join(fragments)

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
