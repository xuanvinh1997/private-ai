"""Ingestion: from a file on disk to searchable chunks.

Three things in here are not refactorable detail and should be changed only on purpose.

*The claim.* Ingestion runs in a worker process while the UI process holds the same
database open, and an in-process ``asyncio.Lock`` cannot span the two. ``document_claims``
is the real lock: one conditional upsert takes it, a heartbeat proves the owner is alive,
and a claim that has gone quiet for longer than the stale window belonged to a process
that was killed and may be taken over.

*The progress ladder.* The percentages written to ``jobs`` are what the documents view
animates. They are fixed points, not estimates, so a document that stalls at 0.48 is
telling the reader exactly which stage failed.

*The integrity guard.* ``indexed_at`` alone never proves an index exists. Before a
document is allowed to read as ready the chunk table is asked whether any row is still
missing its vector, so a run killed halfway can never masquerade as complete.
"""

from __future__ import annotations

import asyncio
import hashlib
import os
import shutil
import socket
from collections.abc import AsyncIterator, Callable
from contextlib import asynccontextmanager, suppress
from datetime import UTC, datetime, timedelta
from pathlib import Path
from time import monotonic
from typing import TYPE_CHECKING, Any
from uuid import uuid4

from private_ai.core.preferences import OCR_ENABLED_KEY, read_app_preferences
from private_ai.llm import InsufficientVram, ProviderUnavailable
from private_ai.rag.ingestion.loaders import (
    TEXT_EXTENSIONS,
    loader_for,
    strip_page_markers,
)
from private_ai.rag.ingestion.ocr import MarkItDownConverter, ocr_gap
from private_ai.rag.ingestion.splitters import SectionAwareTextSplitter

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from langchain_core.documents import Document

    from private_ai.config import Settings
    from private_ai.core.database import Database
    from private_ai.core.protocols import ProgressSink
    from private_ai.llm.router import ModelRouter
    from private_ai.rag.stores.graph_store import GraphStore
    from private_ai.rag.stores.sqlite_vectorstore import SqliteVectorStore

__all__ = [
    "CLAIM_HEARTBEAT_SECONDS",
    "CLAIM_STALE_SECONDS",
    "STATUS_EXTRACTED",
    "IngestionPipeline",
]

# Text arrived without needing a parser, but nothing is indexed yet. This state exists so
# that ``ready`` can mean one thing only — chunked, embedded and safe to query. Retrieval
# filters on ``indexed_at``, so a document sitting here is invisible to search rather than
# silently answering out of its raw text.
STATUS_EXTRACTED = "extracted"

# The owner of an ingestion claim refreshes it on this cadence; a claim quieter than the
# stale window belonged to a process that was killed and is free to take over.
CLAIM_HEARTBEAT_SECONDS = 10.0
CLAIM_STALE_SECONDS = 45.0

# Anything that means "the embedding provider let us down" rather than "the code is wrong".
EMBEDDING_FAILURES = (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError)


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _safe_filename(value: str) -> str:
    name = Path(value).name.strip().replace("\x00", "")
    return name or "document"


class IngestionPipeline:
    def __init__(
        self,
        database: Database,
        vectors: SqliteVectorStore,
        graph: GraphStore,
        router: ModelRouter,
        settings: Settings,
    ) -> None:
        self.database = database
        self.vectors = vectors
        self.graph = graph
        self.router = router
        self.settings = settings
        self.converter = MarkItDownConverter(router, vision_model=settings.vision_model)
        self._locks: dict[str, asyncio.Lock] = {}
        # Identifies this process in document_claims. The asyncio locks above only order
        # work inside one event loop; the claim is what keeps two processes apart.
        self._owner = f"{socket.gethostname()}:{os.getpid()}"

    # --- claims -----------------------------------------------------------

    def _acquire_claim(self, document_id: str) -> bool:
        """Take the cross-process ingestion claim on one document.

        Returns False when another *live* process already owns it, which is the signal to
        leave the document alone rather than start a second run that would delete and
        re-embed the chunks the first one is still writing.
        """
        now = datetime.now(UTC)
        stamp = now.isoformat()
        stale_before = (now - timedelta(seconds=CLAIM_STALE_SECONDS)).isoformat()
        with self.database.connection() as connection:
            cursor = connection.execute(
                """
                INSERT INTO document_claims(document_id, owner, claimed_at, renewed_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(document_id) DO UPDATE SET
                    owner = excluded.owner,
                    claimed_at = excluded.claimed_at,
                    renewed_at = excluded.renewed_at
                WHERE document_claims.owner = excluded.owner
                   OR document_claims.renewed_at < ?
                """,
                (document_id, self._owner, stamp, stamp, stale_before),
            )
            return cursor.rowcount > 0

    def _renew_claim(self, document_id: str) -> None:
        self.database.execute(
            "UPDATE document_claims SET renewed_at = ? WHERE document_id = ? AND owner = ?",
            (_now(), document_id, self._owner),
        )

    def _release_claim(self, document_id: str) -> None:
        self.database.execute(
            "DELETE FROM document_claims WHERE document_id = ? AND owner = ?",
            (document_id, self._owner),
        )

    @asynccontextmanager
    async def _claimed(self, document_id: str) -> AsyncIterator[bool]:
        """Serialise ingestion of one document inside this loop *and* across processes."""
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            if not await asyncio.to_thread(self._acquire_claim, document_id):
                yield False
                return

            async def heartbeat() -> None:
                while True:
                    await asyncio.sleep(CLAIM_HEARTBEAT_SECONDS)
                    await asyncio.to_thread(self._renew_claim, document_id)

            beat = asyncio.create_task(heartbeat())
            try:
                yield True
            finally:
                beat.cancel()
                # Released synchronously: this also runs while the task is being cancelled
                # at shutdown, where another await would be interrupted straight away.
                self._release_claim(document_id)

    def _recover_orphaned_jobs(self) -> None:
        """Fail job rows left behind by a process that died mid-ingestion.

        A job cannot outlive the process running it, so anything still ``processing`` at
        startup is a corpse: without this it keeps the UI pinned at whatever percentage it
        died on. Documents still claimed by a live process are left alone, because a worker
        that was just restarted can still be draining its background ingestion.
        """
        now = datetime.now(UTC)
        stale_before = (now - timedelta(seconds=CLAIM_STALE_SECONDS)).isoformat()
        with self.database.connection() as connection:
            connection.execute(
                "DELETE FROM document_claims WHERE renewed_at < ?",
                (stale_before,),
            )
            connection.execute(
                """
                UPDATE jobs SET status = 'failed', error = ?, updated_at = ?
                WHERE status = 'processing'
                  AND document_id NOT IN (SELECT document_id FROM document_claims)
                """,
                ("Tiến trình xử lý dừng giữa chừng, đã xếp lại hàng đợi", now.isoformat()),
            )

    # --- intake -----------------------------------------------------------

    async def add_file(
        self,
        path: str | Path,
        workspace_id: str,
        *,
        use_ocr: bool | None = None,
    ) -> str:
        """Copy a file into the library and register it. Returns the document id.

        A file already in this workspace, byte for byte, is not stored twice: the existing
        document's id comes back instead, so re-uploading is free rather than a second
        parse and a second set of vectors.
        """
        source = Path(path)
        await self._require_workspace(workspace_id)
        byte_size = await asyncio.to_thread(lambda: source.stat().st_size)
        if byte_size > self.settings.max_upload_bytes:
            raise ValueError("Tệp vượt quá giới hạn dung lượng tải lên")

        filename = _safe_filename(source.name)
        document_id = str(uuid4())
        target_dir = self.settings.documents_dir / document_id
        target_path = target_dir / filename
        try:
            digest = await asyncio.to_thread(self._copy_in, source, target_dir, target_path)
        except Exception:
            await asyncio.to_thread(shutil.rmtree, target_dir, ignore_errors=True)
            raise

        duplicate = await self.database.fetch_one_async(
            "SELECT id FROM documents WHERE workspace_id = ? AND sha256 = ?",
            (workspace_id, digest),
        )
        if duplicate:
            await asyncio.to_thread(shutil.rmtree, target_dir, ignore_errors=True)
            return str(duplicate["id"])

        # A text file needs no extraction stage, but it is not queryable until it has
        # been chunked and embedded. STATUS_EXTRACTED says exactly that: the text is in
        # hand, the index is not. Calling it 'ready' here is what used to let a query
        # reach a document with no chunks, which then fell back to reading the whole raw
        # file into the prompt.
        extracted_text: str | None = None
        status = "queued"
        if target_path.suffix.lower() in TEXT_EXTENSIONS:
            extracted_text = await asyncio.to_thread(
                target_path.read_text,
                encoding="utf-8",
                errors="replace",
            )
            status = STATUS_EXTRACTED

        index_mode, graph_model = await self._default_index_mode()
        now = _now()
        await self.database.execute_async(
            """
            INSERT INTO documents(
                id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
                extracted_text, use_ocr, index_mode, graph_model, error, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
            """,
            (
                document_id,
                workspace_id,
                filename,
                None,
                digest,
                byte_size,
                status,
                str(target_path),
                extracted_text,
                None if use_ocr is None else int(use_ocr),
                index_mode,
                graph_model,
                now,
                now,
            ),
        )
        return document_id

    @staticmethod
    def _copy_in(source: Path, target_dir: Path, target_path: Path) -> str:
        """Copy and hash in one pass, so a large upload is read from disk only once."""
        target_dir.mkdir(parents=True, exist_ok=False)
        digest = hashlib.sha256()
        with source.open("rb") as reader, target_path.open("wb") as writer:
            while chunk := reader.read(1024 * 1024):
                digest.update(chunk)
                writer.write(chunk)
        return digest.hexdigest()

    async def index_text(
        self,
        workspace_id: str,
        filename: str,
        content: str,
        *,
        rag_mode: str = "simple",
    ) -> str:
        """Store text that needs no extraction and index it immediately."""
        if not content.strip():
            raise ValueError("Nội dung tài liệu không được để trống")
        if rag_mode not in {"simple", "graph"}:
            raise ValueError("rag_mode phải là simple hoặc graph")
        await self._require_workspace(workspace_id)
        safe_name = _safe_filename(filename)
        payload = content.encode("utf-8")
        digest = hashlib.sha256(payload).hexdigest()
        duplicate = await self.database.fetch_one_async(
            "SELECT id FROM documents WHERE workspace_id = ? AND sha256 = ?",
            (workspace_id, digest),
        )
        if duplicate:
            return str(duplicate["id"])

        document_id = str(uuid4())
        target_dir = self.settings.documents_dir / document_id
        target_path = target_dir / safe_name

        def write() -> None:
            target_dir.mkdir(parents=True, exist_ok=False)
            target_path.write_text(content, encoding="utf-8")

        await asyncio.to_thread(write)
        preferences = await asyncio.to_thread(read_app_preferences, self.database)
        graph_model = preferences.graph_model if rag_mode == "graph" else ""
        now = _now()
        await self.database.execute_async(
            """
            INSERT INTO documents(
                id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
                extracted_text, index_mode, graph_model, error, created_at, updated_at
            ) VALUES (?, ?, ?, 'text/markdown', ?, ?, 'extracted', ?, ?, ?, ?, NULL, ?, ?)
            """,
            (
                document_id,
                workspace_id,
                safe_name,
                digest,
                len(payload),
                str(target_path),
                content,
                rag_mode,
                graph_model or None,
                now,
                now,
            ),
        )
        await self.process(document_id)
        return document_id

    async def _require_workspace(self, workspace_id: str) -> None:
        row = await self.database.fetch_one_async(
            "SELECT id FROM workspaces WHERE id = ?",
            (workspace_id,),
        )
        if not row:
            raise LookupError(f"Không tìm thấy không gian làm việc {workspace_id}")

    async def _default_index_mode(self) -> tuple[str, str | None]:
        preferences = await asyncio.to_thread(read_app_preferences, self.database)
        index_mode = preferences.rag_mode.value
        return index_mode, (preferences.graph_model if index_mode == "graph" else None)

    # --- processing -------------------------------------------------------

    async def process_pending(self, recover: bool = True) -> None:
        """Finish interrupted extraction and index each document in its saved RAG mode.

        ``recover`` belongs to the first sweep after start-up. The worker calls this on a
        poll loop, and re-running the orphan sweep every tick would keep rewriting job rows
        that nothing is wrong with.
        """
        if recover:
            await asyncio.to_thread(self._recover_orphaned_jobs)
        pending = await self.database.fetch_all_async(
            """
            SELECT id FROM documents
            WHERE status IN ('queued', 'extracted', 'processing')
            ORDER BY created_at
            """
        )
        for document in pending:
            await self.process(str(document["id"]))
        # indexed_at alone is not proof of a usable index: a run killed after the timestamp
        # was written, or one whose chunks were replaced underneath it, leaves embeddings
        # missing while the document still reads as ready.
        unindexed = await self.database.fetch_all_async(
            """
            SELECT d.id FROM documents d
            WHERE d.status = 'ready'
              AND d.extracted_text IS NOT NULL
              AND (
                    d.indexed_at IS NULL
                    OR (d.index_mode = 'simple' AND EXISTS (
                            SELECT 1 FROM document_chunks c
                            WHERE c.document_id = d.id
                              AND COALESCE(c.embedding_vector, c.embedding_json) IS NULL
                    ))
              )
            ORDER BY d.created_at
            """
        )
        for document in unindexed:
            await self.process(str(document["id"]))

    async def process(self, document_id: str, *, on_progress: ProgressSink | None = None) -> None:
        """Extract, chunk and index one document, start to finish."""
        async with self._claimed(document_id) as mine:
            if not mine:
                return
            document = await self.database.fetch_one_async(
                "SELECT status, extracted_text FROM documents WHERE id = ?",
                (document_id,),
            )
            if not document:
                return
            # A text upload and ``index_text`` both arrive already extracted; re-reading
            # them would only replace the stored text with an identical copy.
            # Legacy rows written before STATUS_EXTRACTED existed still say 'ready'
            # here, so both spellings count as already extracted.
            already_read = (
                str(document["status"]) in {STATUS_EXTRACTED, "ready"}
                and document["extracted_text"] is not None
            )
            if already_read:
                job_id, created_at = self._create_job(
                    document_id,
                    step="chunking",
                    progress=0.4,
                    detail="Đã đọc nội dung · chuẩn bị chia đoạn",
                )
                self._emit(on_progress, "chunking", 0.4, "Đã đọc nội dung · chuẩn bị chia đoạn")
            else:
                job_id, created_at = self._create_job(document_id)
                self._emit(on_progress, "queued", 0.05, "Đã nhận tệp · đang chờ xử lý")
                vision_model = await self._resolve_vision_model_async()
                if not await self._extract(
                    document_id,
                    vision_model,
                    job_id,
                    created_at,
                    on_progress,
                ):
                    return
            await self._index(document_id, job_id, created_at, on_progress)

    async def _extract(
        self,
        document_id: str,
        vision_model: str,
        job_id: str,
        created_at: str,
        on_progress: ProgressSink | None,
    ) -> bool:
        document = await self.database.fetch_one_async(
            "SELECT source_path FROM documents WHERE id = ?",
            (document_id,),
        )
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
        self._emit(on_progress, "extracting", 0.12, "Đang đọc và trích xuất nội dung")
        self._update_document(document_id, status="processing", error=None)

        ocr_allowed = self.ocr_enabled(document_id)
        try:
            source_path = Path(str(document["source_path"]))
            loader = loader_for(
                source_path,
                ocr=ocr_allowed,
                vision_model=vision_model,
                converter=self.converter,
            )
            loaded: list[Document] = [chunk async for chunk in loader.alazy_load()]
            text = "\n\n".join(chunk.page_content for chunk in loaded)
            self._write_job(
                job_id,
                document_id,
                created_at,
                status="processing",
                progress=0.34,
                payload={"step": "normalizing", "detail": "Đang làm sạch nội dung đã trích xuất"},
            )
            self._emit(on_progress, "normalizing", 0.34, "Đang làm sạch nội dung đã trích xuất")
            normalized = "\n".join(line.rstrip() for line in text.splitlines()).strip()
            # Page markers are structure, not content: a scan whose every page produced
            # nothing still has one marker per page and must not read as extracted.
            if strip_page_markers(normalized):
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
                self._emit(on_progress, "chunking", 0.4, "Đã đọc nội dung · chuẩn bị chia đoạn")
                return True
            error = ocr_gap(ocr_allowed, vision_model)
            self._update_document(
                document_id,
                status="needs_ocr",
                extracted_text=None,
                error=error,
            )
            job_status = "needs_ocr"
        # UnsupportedDocument and every parser's own failure land here alike: whatever went
        # wrong, the user needs the message on the document rather than in a worker log.
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
        self._emit(on_progress, job_status, 1.0, error)
        return False

    # --- indexing ---------------------------------------------------------

    async def _index(
        self,
        document_id: str,
        job_id: str,
        created_at: str,
        on_progress: ProgressSink | None,
    ) -> bool:
        document = await self.database.fetch_one_async(
            "SELECT workspace_id, filename, extracted_text, status, index_mode, graph_model "
            "FROM documents WHERE id = ?",
            (document_id,),
        )
        # The gate is "extraction produced text", not "the document is queryable" — a
        # re-index of an already-ready document has to pass here too, and a fresh upload
        # sits at STATUS_EXTRACTED until this call promotes it.
        if not document or str(document["status"]) not in {STATUS_EXTRACTED, "ready"}:
            return False

        index_mode = str(document["index_mode"] or "simple")
        graph_model = str(document["graph_model"] or "").strip()
        if index_mode == "graph" and not graph_model:
            resolver = getattr(self.graph, "resolve_graph_model", None)
            graph_model = str(resolver() if callable(resolver) else "").strip()
        if index_mode == "graph" and graph_model and graph_model != document["graph_model"]:
            await self.database.execute_async(
                "UPDATE documents SET graph_model = ?, updated_at = ? WHERE id = ?",
                (graph_model, _now(), document_id),
            )

        latest: dict[str, Any] = {
            "step": "chunking",
            "detail": "Đang chia nội dung thành các đoạn có thể tìm kiếm",
            "index_mode": index_mode,
            **({"graph_model": graph_model} if graph_model else {}),
        }

        def report(event: dict[str, Any]) -> None:
            latest.update(event)
            progress = float(event.get("progress", 0.45))
            self._write_job(
                job_id,
                document_id,
                created_at,
                status="processing",
                progress=progress,
                payload=latest,
            )
            self._emit(
                on_progress,
                str(latest.get("step", "indexing")),
                progress,
                str(latest.get("detail", "")),
            )

        report({"progress": 0.45})
        text = str(document["extracted_text"] or "")
        workspace_id = str(document["workspace_id"])
        if index_mode == "simple":
            indexed = await self._index_simple(
                document_id,
                workspace_id,
                text,
                on_progress=report,
            )
        else:
            indexed = await self._index_graph(
                document_id,
                workspace_id,
                str(document["filename"]),
                text,
                graph_model,
                on_progress=report,
            )

        await self.database.execute_async(
            "UPDATE documents SET indexed_at = ?, status = ?, error = ?, updated_at = ? "
            "WHERE id = ?",
            (
                _now() if indexed else None,
                "ready" if indexed else "failed",
                (
                    None
                    if indexed
                    else "Không thể tạo chỉ mục. Kiểm tra mô hình embedding rồi thử lại."
                ),
                _now(),
                document_id,
            ),
        )
        detail = (
            (
                "Đã tạo xong chỉ mục vector · không dùng LLM"
                if index_mode == "simple"
                else "Đã tạo xong embedding và graph memory"
                + (f" · {graph_model}" if graph_model else "")
            )
            if indexed
            else "Không thể tạo chỉ mục"
        )
        self._write_job(
            job_id,
            document_id,
            created_at,
            status="completed" if indexed else "failed",
            progress=1.0,
            payload={**latest, "step": "completed" if indexed else "failed", "detail": detail},
            error=None if indexed else "Không thể tạo chỉ mục",
        )
        self._emit(on_progress, "completed" if indexed else "failed", 1.0, detail)
        return indexed

    async def _index_simple(
        self,
        document_id: str,
        workspace_id: str,
        text: str,
        *,
        on_progress: Callable[[dict[str, Any]], None],
    ) -> bool:
        """Build only chunk embeddings. This path never calls a language model."""
        splitter = SectionAwareTextSplitter.from_settings(self.settings)
        chunks = splitter.split_marked_text(
            text,
            metadata={"document_id": document_id, "workspace_id": workspace_id},
        )
        if not chunks or not self._embedding_model():
            return False

        await asyncio.to_thread(self._replace_sections, document_id, chunks)
        total = len(chunks)
        on_progress(
            {
                "step": "embedding",
                "progress": 0.48,
                "detail": f"Đang tạo embedding cho {total} đoạn",
                "estimated_chunks": total,
                "embedded_vectors": 0,
                "engine": "vector",
            }
        )

        preferences = await asyncio.to_thread(read_app_preferences, self.database)
        batch_size = max(1, preferences.embedding_batch_size)
        concurrency = max(1, preferences.embedding_concurrency)
        store = self.vectors.scoped(workspace_id)
        started_at = monotonic()
        embedded = 0
        progress_lock = asyncio.Lock()
        semaphore = asyncio.Semaphore(concurrency)

        async def embed_batch(batch: list[Document]) -> None:
            nonlocal embedded
            async with semaphore:
                await store.aadd_documents(batch)
            async with progress_lock:
                embedded += len(batch)
                elapsed = max(monotonic() - started_at, 0.001)
                on_progress(
                    {
                        "step": "embedding",
                        "progress": 0.48 + (embedded / total) * 0.48,
                        "detail": f"Đã tạo {embedded}/{total} vector",
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
                    for offset in range(0, total, batch_size)
                )
            )
        except EMBEDDING_FAILURES:
            return False
        if embedded != total:
            return False
        # The counter only knows what this run wrote. Ask the table before letting the
        # caller stamp indexed_at, so a half-embedded document can never read as ready.
        missing = await self.database.fetch_one_async(
            "SELECT COUNT(*) AS missing FROM document_chunks "
            "WHERE document_id = ? AND COALESCE(embedding_vector, embedding_json) IS NULL",
            (document_id,),
        )
        return int(missing["missing"]) == 0 if missing else False

    async def _index_graph(
        self,
        document_id: str,
        workspace_id: str,
        filename: str,
        text: str,
        graph_model: str,
        *,
        on_progress: Callable[[dict[str, Any]], None],
    ) -> bool:
        try:
            result = await self.graph.index_document(
                workspace_id,
                document_id,
                filename,
                text,
                on_progress=on_progress,
                graph_model=graph_model,
            )
        except (InsufficientVram, ProviderUnavailable):
            return False
        # The store's contract returns None on success; an explicit False is a refusal.
        return result is not False

    def _replace_sections(self, document_id: str, chunks: list[Document]) -> None:
        """Rewrite this document's sections, and clear the chunks that pointed at them.

        Sections have to exist before the vector store inserts chunks, because a chunk row
        carries the section id it belongs to.
        """
        created_at = _now()
        sections: dict[int, dict[str, Any]] = {}
        for chunk in chunks:
            metadata = chunk.metadata
            section_index = int(metadata["section_index"])
            section = sections.setdefault(
                section_index,
                {
                    "id": f"{document_id}:section:{section_index}",
                    "title": metadata.get("section_title", ""),
                    "level": metadata.get("section_level", 0),
                    "pages": [],
                },
            )
            metadata["section_id"] = section["id"]
            page_number = metadata.get("page_number")
            if page_number is not None:
                section["pages"].append(int(page_number))
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

    # --- deletion ---------------------------------------------------------

    async def delete_document(self, document_id: str, *, confirmed: bool = False) -> None:
        if not confirmed:
            raise PermissionError("Xóa tài liệu cần confirmed=true")
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            document = await self.database.fetch_one_async(
                "SELECT workspace_id, source_path FROM documents WHERE id = ?",
                (document_id,),
            )
            if not document:
                return
            with suppress(Exception):
                await self.graph.delete_document(str(document["workspace_id"]), document_id)
            with suppress(Exception):
                await self.vectors.adelete_document(document_id)
            await self.database.execute_async(
                "DELETE FROM documents WHERE id = ?",
                (document_id,),
            )
            source_path = Path(str(document["source_path"]))
            await asyncio.to_thread(shutil.rmtree, source_path.parent, ignore_errors=True)

    # --- models and OCR ---------------------------------------------------

    def resolve_vision_model(self) -> str:
        """The model OCR reads with: the explicit pick, else the configured default."""
        stored = self.database.fetch_one(
            "SELECT model_name FROM model_defaults WHERE task = 'vision'"
        )
        if stored and str(stored["model_name"]).strip():
            return str(stored["model_name"]).strip()
        return self.settings.vision_model.strip()

    async def _resolve_vision_model_async(self) -> str:
        """The same pick, plus a look at what the provider actually offers.

        Ticking OCR is the whole instruction, so a provider that already serves a
        vision-capable model should not need a second, separate choice.
        """
        chosen = await asyncio.to_thread(self.resolve_vision_model)
        if chosen:
            return chosen
        try:
            models = await self.router.list_models()
        except (ProviderUnavailable, OSError):
            return ""
        return next((model.name for model in models if "vision" in model.capabilities), "")

    def ocr_enabled(self, document_id: str) -> bool:
        """Whether reading may fall back to OCR: the document's own choice, else the default."""
        if document_id:
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

    def _embedding_model(self) -> str:
        name = str(getattr(self.vectors, "embedding_model", "") or "").strip()
        if name:
            return name
        return (self.router.default_model("embedding") or self.settings.embedding_model).strip()

    # --- jobs -------------------------------------------------------------

    @staticmethod
    def _emit(sink: ProgressSink | None, stage: str, progress: float, detail: str = "") -> None:
        if sink is None:
            return
        # A UI callback must never be able to abort an ingestion that has already succeeded.
        with suppress(Exception):
            sink(stage, progress, detail)

    def _create_job(
        self,
        document_id: str,
        *,
        step: str = "queued",
        progress: float = 0.05,
        detail: str = "Đã nhận tệp · đang chờ xử lý",
    ) -> tuple[str, str]:
        job_id = str(uuid4())
        created_at = _now()
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
        payload: dict[str, Any],
        error: str | None = None,
    ) -> None:
        job_payload: dict[str, Any] = {"document_id": document_id, **payload}
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
                "updated_at": _now(),
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
            (status, extracted_text, error, _now(), document_id),
        )
