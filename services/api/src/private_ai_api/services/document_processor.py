from __future__ import annotations

import asyncio
import json
import math
import re
import shutil
import threading
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from time import monotonic
from uuid import uuid4
from xml.etree import ElementTree
from zipfile import BadZipFile, ZipFile

from markitdown import MarkItDown
from openai import OpenAI
from pypdf import PdfReader

from private_ai_api.database import Database
from private_ai_api.services.app_preferences import OCR_ENABLED_KEY
from private_ai_api.services.gpu_lease import InsufficientVram
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
        """Finish interrupted extraction and index each document in its saved RAG mode."""
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
            job_id, created_at = self._create_job(document_id)
            vision_model = await self.resolve_vision_model()
            extracted = await asyncio.to_thread(
                self._process_sync,
                document_id,
                vision_model,
                job_id,
                created_at,
            )
            if extracted:
                await self._index_document_unlocked(document_id, job_id, created_at)

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
        """Index extracted text as vectors only or as a full LightRAG graph."""
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            job_id, created_at = self._create_job(
                document_id,
                step="chunking",
                progress=0.4,
                detail="Đã đọc nội dung · chuẩn bị chia đoạn",
            )
            return await self._index_document_unlocked(document_id, job_id, created_at)

    async def _index_document_unlocked(
        self,
        document_id: str,
        job_id: str,
        created_at: str,
    ) -> bool:
        document = self.database.fetch_one(
            "SELECT workspace_id, filename, extracted_text, status, index_mode, graph_model "
            "FROM documents WHERE id = ?",
            (document_id,),
        )
        if not document or document["status"] != "ready":
            return False

        index_mode = str(document["index_mode"] or "simple")
        graph_model = str(document["graph_model"] or "").strip()
        if index_mode == "graph" and not graph_model:
            resolver = getattr(self.lightrag, "resolve_graph_model", None)
            graph_model = str(resolver() if callable(resolver) else "").strip()
        if index_mode == "graph" and graph_model and graph_model != document["graph_model"]:
            self.database.execute(
                "UPDATE documents SET graph_model = ?, updated_at = ? WHERE id = ?",
                (graph_model, datetime.now(UTC).isoformat(), document_id),
            )
        latest: dict[str, object] = {
            "step": "chunking",
            "detail": "Đang chia nội dung thành các đoạn có thể tìm kiếm",
            "index_mode": index_mode,
            **({"graph_model": graph_model} if graph_model else {}),
        }

        def report(event: dict[str, object]) -> None:
            latest.update(event)
            self._write_job(
                job_id,
                document_id,
                created_at,
                status="processing",
                progress=float(event.get("progress", 0.45)),
                payload=latest,
            )

        if index_mode == "simple":
            indexed = await self._index_simple_document(
                document_id,
                str(document["extracted_text"] or ""),
                on_progress=report,
            )
        else:
            indexed = await self.lightrag.index_document(
                str(document["workspace_id"]),
                document_id,
                str(document["filename"]),
                str(document["extracted_text"] or ""),
                on_progress=report,
                graph_model=graph_model,
            )
        indexed_at = datetime.now(UTC).isoformat() if indexed else None
        self.database.execute(
            "UPDATE documents SET indexed_at = ?, status = ?, error = ?, updated_at = ? "
            "WHERE id = ?",
            (
                indexed_at,
                "ready" if indexed else "failed",
                (
                    None
                    if indexed
                    else "Không thể tạo chỉ mục. Kiểm tra mô hình embedding rồi thử lại."
                ),
                datetime.now(UTC).isoformat(),
                document_id,
            ),
        )
        self._write_job(
            job_id,
            document_id,
            created_at,
            status="completed" if indexed else "failed",
            progress=1.0,
            payload={
                **latest,
                "step": "completed" if indexed else "failed",
                "detail": (
                    (
                        "Đã tạo xong chỉ mục vector · không dùng LLM"
                        if index_mode == "simple"
                        else "Đã tạo xong embedding và graph memory"
                        + (f" · {graph_model}" if graph_model else "")
                    )
                    if indexed
                    else "Không thể tạo chỉ mục"
                ),
            },
            error=None if indexed else "Không thể tạo chỉ mục",
        )
        return indexed

    async def _index_simple_document(
        self,
        document_id: str,
        text: str,
        *,
        on_progress: Callable[[dict[str, object]], None],
    ) -> bool:
        """Build only chunk embeddings. This path never calls a language model."""
        records = self._chunk_records(text)
        if not records or self.ai is None:
            return False
        embedding_model = str(getattr(self.lightrag, "embedding_model", "")).strip()
        if not embedding_model:
            return False

        self._replace_simple_chunks(document_id, records)
        total = len(records)
        on_progress(
            {
                "step": "embedding",
                "progress": 0.48,
                "detail": f"Đang tạo embedding cho {total} đoạn · không chạy LLM",
                "estimated_chunks": total,
                "embedded_vectors": 0,
                "engine": "vector",
            }
        )
        chunks = self.database.fetch_all(
            "SELECT id, content FROM document_chunks WHERE document_id = ? ORDER BY chunk_index",
            (document_id,),
        )
        batch_size = max(1, int(getattr(self.lightrag, "embedding_batch_size", 32) or 32))
        concurrency = max(
            1,
            int(getattr(self.lightrag, "embedding_concurrency", 4) or 4),
        )
        started_at = monotonic()
        embedded = 0
        progress_lock = asyncio.Lock()
        semaphore = asyncio.Semaphore(concurrency)

        async def embed_batch(batch: list[dict[str, object]]) -> None:
            nonlocal embedded
            async with semaphore:
                vectors = await self.ai.embed(
                    embedding_model,
                    [str(chunk["content"]) for chunk in batch],
                )
            self.database.execute_many(
                "UPDATE document_chunks SET embedding_json = ?, embedding_model = ? "
                "WHERE id = ?",
                (
                    (
                        json.dumps(vector, separators=(",", ":")),
                        embedding_model,
                        chunk["id"],
                    )
                    for chunk, vector in zip(batch, vectors, strict=True)
                ),
            )
            async with progress_lock:
                embedded += len(batch)
                elapsed = max(monotonic() - started_at, 0.001)
                on_progress(
                    {
                        "step": "embedding",
                        "progress": 0.48 + (embedded / total) * 0.48,
                        "detail": f"Đã tạo {embedded}/{total} vector · không chạy LLM",
                        "estimated_chunks": total,
                        "embedded_vectors": embedded,
                        "vectors_per_second": embedded / elapsed,
                        "elapsed_seconds": elapsed,
                        "engine": "vector",
                    }
                )

        try:
            await asyncio.gather(
                *(
                    embed_batch(chunks[offset : offset + batch_size])
                    for offset in range(0, len(chunks), batch_size)
                )
            )
        except (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError):
            return False
        return embedded == total

    def _replace_simple_chunks(
        self,
        document_id: str,
        records: list[dict[str, object]],
    ) -> None:
        created_at = datetime.now(UTC).isoformat()
        sections: dict[int, dict[str, object]] = {}
        for record in records:
            section_index = int(record["section_index"])
            section = sections.setdefault(
                section_index,
                {
                    "id": f"{document_id}:section:{section_index}",
                    "title": record["section_title"],
                    "level": record["section_level"],
                    "pages": [],
                },
            )
            if record["page_number"] is not None:
                pages = section["pages"]
                if isinstance(pages, list):
                    pages.append(int(record["page_number"]))
        with self.database.connection() as connection:
            connection.execute(
                "DELETE FROM document_chunks WHERE document_id = ?",
                (document_id,),
            )
            connection.execute(
                "DELETE FROM document_sections WHERE document_id = ?",
                (document_id,),
            )
            connection.executemany(
                """
                INSERT INTO document_sections(
                    id, document_id, section_index, title, level, page_start, page_end, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    (
                        section["id"],
                        document_id,
                        section_index,
                        section["title"],
                        section["level"],
                        min(section["pages"]) if section["pages"] else None,
                        max(section["pages"]) if section["pages"] else None,
                        created_at,
                    )
                    for section_index, section in sorted(sections.items())
                ),
            )
            connection.executemany(
                """
                INSERT INTO document_chunks(
                    id, document_id, chunk_index, content, section_id, section_title,
                    section_level, page_number, embedding_json, embedding_model, graph_model,
                    created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?)
                """,
                (
                    (
                        str(uuid4()),
                        document_id,
                        index,
                        record["content"],
                        sections[int(record["section_index"])]["id"],
                        record["section_title"],
                        record["section_level"],
                        record["page_number"],
                        created_at,
                    )
                    for index, record in enumerate(records)
                ),
            )

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
        mode: str = "simple",
    ) -> list[dict[str, object]]:
        """Search one workspace. Never returns another workspace's content."""
        local = await self._search_simple(query, workspace_id, max(limit, 10))
        retrieval_mode = "naive" if mode == "simple" else "mix"
        graph = await self.lightrag.search(
            query,
            workspace_id,
            max(limit, 10),
            mode=retrieval_mode,
        )
        ordered = [*local, *graph] if mode == "simple" else [*graph, *local]
        return self._deduplicate_results(ordered, limit)

    async def _search_simple(
        self,
        query: str,
        workspace_id: str,
        limit: int,
    ) -> list[dict[str, object]]:
        rows = self.database.fetch_all(
            """
            SELECT c.id AS chunk_id, c.content, c.chunk_index, c.embedding_json,
                   c.embedding_model, d.id AS document_id, d.filename
            FROM document_chunks AS c
            JOIN documents AS d ON d.id = c.document_id
            WHERE d.workspace_id = ? AND d.status = 'ready' AND d.index_mode = 'simple'
            ORDER BY d.created_at DESC, c.chunk_index
            """,
            (workspace_id,),
        )
        if not rows:
            return []
        tokens = list(dict.fromkeys(self._search_tokens(query)))[:32]
        query_vector: list[float] = []
        embedding_model = str(getattr(self.lightrag, "embedding_model", "")).strip()
        if self.ai is not None and embedding_model and query.strip():
            try:
                query_vector = (await self.ai.embed(embedding_model, [query]))[0]
            except (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError):
                query_vector = []

        ranked: list[tuple[float, dict[str, object]]] = []
        for row in rows:
            searchable = self._search_tokens(f"{row['filename']} {row['content']}")
            matched = len(set(tokens) & set(searchable))
            keyword_score = matched / max(1, len(tokens))
            semantic_score = -1.0
            if (
                query_vector
                and row["embedding_model"] == embedding_model
                and row["embedding_json"]
            ):
                semantic_score = self._cosine_similarity(
                    query_vector,
                    json.loads(str(row["embedding_json"])),
                )
            if keyword_score <= 0 and semantic_score < 0.3:
                continue
            score = max(keyword_score, 0.0) + max(semantic_score, 0.0)
            ranked.append(
                (
                    score,
                    {
                        "chunk_id": row["chunk_id"],
                        "document_id": row["document_id"],
                        "filename": row["filename"],
                        "content": row["content"],
                        "score": round(score, 4),
                    },
                )
            )
        ranked.sort(key=lambda item: -item[0])
        return [record for _, record in ranked[: max(1, min(limit, 20))]]

    @staticmethod
    def _deduplicate_results(
        rows: list[dict[str, object]],
        limit: int,
    ) -> list[dict[str, object]]:
        selected: list[dict[str, object]] = []
        seen: set[tuple[str, str]] = set()
        for row in rows:
            key = (str(row.get("filename") or ""), str(row.get("content") or ""))
            if key in seen:
                continue
            seen.add(key)
            selected.append(row)
            if len(selected) >= max(1, min(limit, 20)):
                break
        return selected

    @staticmethod
    def _cosine_similarity(left: list[float], right: list[float]) -> float:
        if not left or len(left) != len(right):
            return -1.0
        dot = sum(a * b for a, b in zip(left, right, strict=True))
        left_norm = math.sqrt(sum(value * value for value in left))
        right_norm = math.sqrt(sum(value * value for value in right))
        if not left_norm or not right_norm:
            return -1.0
        return dot / (left_norm * right_norm)

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

    @staticmethod
    def _split_text(text: str, size: int = 1400, overlap: int = 180) -> list[str]:
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

    @classmethod
    def _chunk_records(
        cls,
        text: str,
        size: int = 1400,
        overlap: int = 180,
    ) -> list[dict[str, object]]:
        records: list[dict[str, object]] = []
        section_index = 0
        section_title = "Nội dung"
        section_level = 0
        page_number: int | None = None
        buffer: list[str] = []

        def flush() -> None:
            content = "\n".join(buffer).strip()
            buffer.clear()
            for chunk in cls._split_text(content, size=size, overlap=overlap):
                records.append(
                    {
                        "content": chunk,
                        "section_index": section_index,
                        "section_title": section_title,
                        "section_level": section_level,
                        "page_number": page_number,
                    }
                )

        for raw_line in text.splitlines():
            line = raw_line.rstrip()
            page_match = PAGE_MARKER.fullmatch(line.strip())
            if page_match:
                flush()
                page_number = int(page_match.group(1))
                continue
            heading = re.match(r"^(#{1,6})\s+(.+?)\s*$", line)
            if heading:
                flush()
                if records or section_title != "Nội dung":
                    section_index += 1
                section_title = heading.group(2).strip()[:240]
                section_level = len(heading.group(1))
                buffer.append(line)
                continue
            buffer.append(line)
        flush()
        return records

    @staticmethod
    def _search_tokens(value: str) -> list[str]:
        return [
            token
            for token in re.findall(r"[^\W_]+", value.casefold(), flags=re.UNICODE)
            if len(token) > 1
        ]

    def _process_sync(
        self,
        document_id: str,
        vision_model: str,
        job_id: str,
        created_at: str,
    ) -> bool:
        document = self.database.fetch_one("SELECT * FROM documents WHERE id = ?", (document_id,))
        if not document:
            return False
        self._write_job(
            job_id,
            document_id,
            created_at,
            status="processing",
            progress=0.12,
            payload={"step": "extracting", "detail": "Đang đọc và trích xuất nội dung"},
        )
        self._update_document(document_id, status="processing", error=None)
        try:
            source_path = Path(document["source_path"])
            ocr_allowed = self.ocr_enabled(document_id)
            text = self._extract(source_path, ocr_allowed, vision_model)
            self._write_job(
                job_id,
                document_id,
                created_at,
                status="processing",
                progress=0.34,
                payload={"step": "normalizing", "detail": "Đang làm sạch nội dung đã trích xuất"},
            )
            normalized = "\n".join(line.rstrip() for line in text.splitlines()).strip()
            meaningful = "\n".join(
                line for line in normalized.splitlines() if not PAGE_MARKER.fullmatch(line.strip())
            ).strip()
            if meaningful:
                self._update_document(
                    document_id,
                    status="ready",
                    extracted_text=normalized,
                    error=None,
                )
                self._write_job(
                    job_id,
                    document_id,
                    created_at,
                    status="processing",
                    progress=0.4,
                    payload={"step": "chunking", "detail": "Đã đọc nội dung · chuẩn bị chia đoạn"},
                )
                return True
            else:
                error = self._ocr_gap(ocr_allowed, vision_model)
                self._update_document(
                    document_id,
                    status="needs_ocr",
                    extracted_text=None,
                    error=error,
                )
                job_status = "needs_ocr"
        except Exception as exc:
            self._update_document(
                document_id,
                status="failed",
                extracted_text=None,
                error=str(exc),
            )
            job_status = "failed"
            error = str(exc)
        self._write_job(
            job_id,
            document_id,
            created_at,
            status=job_status,
            progress=1.0,
            payload={"step": job_status, "detail": error},
            error=error,
        )
        return False

    def _create_job(
        self,
        document_id: str,
        *,
        step: str = "queued",
        progress: float = 0.05,
        detail: str = "Đã nhận tệp · đang chờ xử lý",
    ) -> tuple[str, str]:
        job_id = str(uuid4())
        created_at = datetime.now(UTC).isoformat()
        self._write_job(
            job_id,
            document_id,
            created_at,
            status="processing",
            progress=progress,
            payload={"step": step, "detail": detail},
        )
        return job_id, created_at

    def _write_job(
        self,
        job_id: str,
        document_id: str,
        created_at: str,
        *,
        status: str,
        progress: float,
        payload: dict[str, object],
        error: str | None = None,
    ) -> None:
        job_payload = {"document_id": document_id, **payload}
        if "index_mode" not in job_payload:
            document = self.database.fetch_one(
                "SELECT index_mode, graph_model FROM documents WHERE id = ?",
                (document_id,),
            )
            job_payload["index_mode"] = (
                str(document["index_mode"] or "simple") if document else "simple"
            )
            if document and document["graph_model"]:
                job_payload["graph_model"] = str(document["graph_model"])
        self.database.upsert_job(
            {
                "id": job_id,
                "document_id": document_id,
                "kind": "document_ingestion",
                "status": status,
                "progress": max(0.0, min(progress, 1.0)),
                "payload": job_payload,
                "error": error,
                "created_at": created_at,
                "updated_at": datetime.now(UTC).isoformat(),
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
