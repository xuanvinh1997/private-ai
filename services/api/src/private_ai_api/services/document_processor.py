from __future__ import annotations

import asyncio
import json
import math
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
from private_ai_api.services.gpu_lease import InsufficientVram
from private_ai_api.services.ollama import OllamaClient, OllamaUnavailable

if __import__("typing").TYPE_CHECKING:
    from private_ai_api.services.graph_store import GraphStore

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


class OcrUnavailable(RuntimeError):
    pass


class DocumentProcessor:
    def __init__(
        self,
        database: Database,
        ollama: OllamaClient,
        *,
        embedding_model: str,
        embedding_enabled: bool,
        graph_store: GraphStore | None = None,
        graph_entity_model: str = "",
        ollama_url: str = "http://127.0.0.1:11434",
        vision_model: str = "",
    ) -> None:
        self.database = database
        self.ollama = ollama
        self.embedding_model = embedding_model
        self.embedding_enabled = embedding_enabled
        self.graph_store = graph_store
        self.graph_entity_model = graph_entity_model.strip()
        self.ollama_url = ollama_url.rstrip("/")
        self.vision_model = vision_model.strip()
        self._markitdown: MarkItDown | None = None
        self._markitdown_model: str | None = None
        self._markitdown_lock = threading.RLock()
        self._locks: dict[str, asyncio.Lock] = {}

    async def process_pending(self) -> None:
        pending = self.database.fetch_all(
            """
            SELECT id, status, extracted_text FROM documents
            WHERE status IN ('queued', 'processing')
               OR (
                    status = 'ready'
                    AND extracted_text IS NOT NULL
                    AND NOT EXISTS (
                        SELECT 1 FROM document_chunks WHERE document_id = documents.id
                    )
               )
            ORDER BY created_at
            """
        )
        for document in pending:
            if document["status"] == "ready" and document["extracted_text"]:
                await asyncio.to_thread(
                    self.index_text,
                    document["id"],
                    document["extracted_text"],
                )
            else:
                await self.process(document["id"])
        if self.embedding_enabled:
            missing_embeddings = self.database.fetch_all(
                """
                SELECT DISTINCT d.id
                FROM documents AS d
                JOIN document_chunks AS c ON c.document_id = d.id
                WHERE d.status = 'ready'
                  AND (c.embedding_json IS NULL OR c.embedding_model != ?)
                """,
                (self.embedding_model,),
            )
            for document in missing_embeddings:
                await self.embed_document(document["id"])
        if self.graph_entity_model:
            missing_graph = self.database.fetch_all(
                """
                SELECT DISTINCT d.id
                FROM documents AS d
                JOIN document_chunks AS c ON c.document_id = d.id
                WHERE d.status = 'ready'
                  AND (c.graph_model IS NULL OR c.graph_model != ?)
                ORDER BY d.updated_at DESC
                """,
                (self.graph_entity_model,),
            )
            for document in missing_graph:
                await self.extract_graph(str(document["id"]))

    async def process(self, document_id: str) -> None:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            await asyncio.to_thread(self._process_sync, document_id)
            await self._embed_document(document_id)
            await self._extract_graph_document(document_id)
            await self._sync_graph(document_id)

    async def delete(self, document_id: str) -> bool:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            if self.graph_store:
                await self.graph_store.delete_document(document_id)
            return await asyncio.to_thread(self._delete_sync, document_id)

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

    def index_text(self, document_id: str, text: str) -> None:
        chunks = self._chunk_records(text)
        created_at = datetime.now(UTC).isoformat()
        section_rows: dict[int, dict[str, object]] = {}
        for chunk in chunks:
            section_index = int(chunk["section_index"])
            section = section_rows.setdefault(
                section_index,
                {
                    "id": f"{document_id}:section:{section_index}",
                    "title": chunk["section_title"],
                    "level": chunk["section_level"],
                    "pages": [],
                },
            )
            if chunk["page_number"] is not None:
                section["pages"].append(int(chunk["page_number"]))  # type: ignore[union-attr]
        with self.database.connection() as connection:
            connection.execute("DELETE FROM document_chunks WHERE document_id = ?", (document_id,))
            connection.execute(
                "DELETE FROM document_sections WHERE document_id = ?",
                (document_id,),
            )
            connection.executemany(
                """
                INSERT INTO document_sections(
                    id, document_id, section_index, title, level,
                    page_start, page_end, created_at
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
                    for section_index, section in sorted(section_rows.items())
                ),
            )
            connection.executemany(
                """
                INSERT INTO document_chunks(
                    id, document_id, chunk_index, content,
                    section_id, section_title, section_level, page_number,
                    embedding_json, embedding_model, created_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)
                """,
                (
                    (
                        str(uuid4()),
                        document_id,
                        index,
                        chunk["content"],
                        section_rows[int(chunk["section_index"])]["id"],
                        chunk["section_title"],
                        chunk["section_level"],
                        chunk["page_number"],
                        created_at,
                    )
                    for index, chunk in enumerate(chunks)
                ),
            )

    async def embed_document(self, document_id: str) -> bool:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            embedded = await self._embed_document(document_id)
            await self._extract_graph_document(document_id)
            await self._sync_graph(document_id)
            return embedded

    async def extract_graph(self, document_id: str) -> bool:
        lock = self._locks.setdefault(document_id, asyncio.Lock())
        async with lock:
            extracted = await self._extract_graph_document(document_id)
            await self._sync_graph(document_id)
            return extracted

    async def _sync_graph(self, document_id: str) -> bool:
        if not self.graph_store:
            return False
        return await self.graph_store.sync_document(document_id)

    async def _embed_document(self, document_id: str) -> bool:
        if not self.embedding_enabled:
            return False
        chunks = self.database.fetch_all(
            """
            SELECT c.id, c.content
            FROM document_chunks AS c
            JOIN documents AS d ON d.id = c.document_id
            WHERE c.document_id = ? AND d.status = 'ready'
              AND (c.embedding_json IS NULL OR c.embedding_model != ?)
            ORDER BY c.chunk_index
            """,
            (document_id, self.embedding_model),
        )
        if not chunks:
            return True
        job_id = str(uuid4())
        created_at = datetime.now(UTC).isoformat()
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "document_embedding",
                "status": "processing",
                "progress": 0.0,
                "payload": {"document_id": document_id, "model": self.embedding_model},
                "created_at": created_at,
                "updated_at": created_at,
            }
        )
        try:
            for offset in range(0, len(chunks), 16):
                batch = chunks[offset : offset + 16]
                vectors = await self.ollama.embed(
                    self.embedding_model,
                    [str(chunk["content"]) for chunk in batch],
                )
                self.database.execute_many(
                    """
                    UPDATE document_chunks
                    SET embedding_json = ?, embedding_model = ?
                    WHERE id = ?
                    """,
                    (
                        (
                            json.dumps(vector, separators=(",", ":")),
                            self.embedding_model,
                            chunk["id"],
                        )
                        for chunk, vector in zip(batch, vectors, strict=True)
                    ),
                )
                self.database.upsert_job(
                    {
                        "id": job_id,
                        "kind": "document_embedding",
                        "status": "processing",
                        "progress": min(1.0, (offset + len(batch)) / len(chunks)),
                        "payload": {
                            "document_id": document_id,
                            "model": self.embedding_model,
                        },
                        "created_at": created_at,
                        "updated_at": datetime.now(UTC).isoformat(),
                    }
                )
        except (InsufficientVram, OllamaUnavailable, ValueError, TypeError) as exc:
            self.database.upsert_job(
                {
                    "id": job_id,
                    "kind": "document_embedding",
                    "status": "failed",
                    "progress": 0.0,
                    "payload": {"document_id": document_id, "model": self.embedding_model},
                    "error": str(exc),
                    "created_at": created_at,
                    "updated_at": datetime.now(UTC).isoformat(),
                }
            )
            return False
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "document_embedding",
                "status": "completed",
                "progress": 1.0,
                "payload": {"document_id": document_id, "model": self.embedding_model},
                "created_at": created_at,
                "updated_at": datetime.now(UTC).isoformat(),
            }
        )
        return True

    async def _extract_graph_document(self, document_id: str) -> bool:
        if not self.graph_entity_model:
            return False
        chunks = self.database.fetch_all(
            """
            SELECT c.id, c.content
            FROM document_chunks AS c
            JOIN documents AS d ON d.id = c.document_id
            WHERE c.document_id = ? AND d.status = 'ready'
              AND (c.graph_model IS NULL OR c.graph_model != ?)
            ORDER BY c.chunk_index
            """,
            (document_id, self.graph_entity_model),
        )
        if not chunks:
            return True
        job_id = str(uuid4())
        created_at = datetime.now(UTC).isoformat()
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "graph_extraction",
                "status": "processing",
                "progress": 0.0,
                "payload": {"document_id": document_id, "model": self.graph_entity_model},
                "created_at": created_at,
                "updated_at": created_at,
            }
        )
        try:
            for index, chunk in enumerate(chunks, start=1):
                facts = await self.ollama.extract_graph(
                    self.graph_entity_model,
                    str(chunk["content"]),
                )
                extracted_at = datetime.now(UTC).isoformat()
                with self.database.connection() as connection:
                    connection.execute(
                        "DELETE FROM chunk_entities WHERE chunk_id = ?",
                        (chunk["id"],),
                    )
                    connection.execute(
                        "DELETE FROM chunk_relations WHERE chunk_id = ?",
                        (chunk["id"],),
                    )
                    connection.executemany(
                        """
                        INSERT INTO chunk_entities(
                            chunk_id, document_id, key, name, kind, source_model, created_at
                        ) VALUES (?, ?, ?, ?, ?, ?, ?)
                        """,
                        (
                            (
                                chunk["id"],
                                document_id,
                                entity["key"],
                                entity["name"],
                                entity["kind"],
                                self.graph_entity_model,
                                extracted_at,
                            )
                            for entity in facts["entities"]
                        ),
                    )
                    connection.executemany(
                        """
                        INSERT INTO chunk_relations(
                            chunk_id, document_id, source_key, target_key,
                            relation, source_model, created_at
                        ) VALUES (?, ?, ?, ?, ?, ?, ?)
                        """,
                        (
                            (
                                chunk["id"],
                                document_id,
                                relation["source_key"],
                                relation["target_key"],
                                relation["relation"],
                                self.graph_entity_model,
                                extracted_at,
                            )
                            for relation in facts["relations"]
                        ),
                    )
                    connection.execute(
                        "UPDATE document_chunks SET graph_model = ? WHERE id = ?",
                        (self.graph_entity_model, chunk["id"]),
                    )
                self.database.upsert_job(
                    {
                        "id": job_id,
                        "kind": "graph_extraction",
                        "status": "processing",
                        "progress": index / len(chunks),
                        "payload": {
                            "document_id": document_id,
                            "model": self.graph_entity_model,
                        },
                        "created_at": created_at,
                        "updated_at": datetime.now(UTC).isoformat(),
                    }
                )
        except (InsufficientVram, OllamaUnavailable, KeyError, TypeError, ValueError) as exc:
            self.database.upsert_job(
                {
                    "id": job_id,
                    "kind": "graph_extraction",
                    "status": "failed",
                    "progress": 0.0,
                    "payload": {
                        "document_id": document_id,
                        "model": self.graph_entity_model,
                    },
                    "error": str(exc),
                    "created_at": created_at,
                    "updated_at": datetime.now(UTC).isoformat(),
                }
            )
            return False
        self.database.upsert_job(
            {
                "id": job_id,
                "kind": "graph_extraction",
                "status": "completed",
                "progress": 1.0,
                "payload": {"document_id": document_id, "model": self.graph_entity_model},
                "created_at": created_at,
                "updated_at": datetime.now(UTC).isoformat(),
            }
        )
        return True

    async def search(self, query: str, limit: int = 5) -> list[dict[str, object]]:
        tokens = list(dict.fromkeys(self._search_tokens(query)))[:32]
        if not tokens:
            return []
        rows = self.database.fetch_all(
            """
            SELECT c.id AS chunk_id, c.document_id, c.chunk_index, c.content,
                   c.section_id, c.section_title, c.section_level, c.page_number,
                   c.embedding_json, c.embedding_model, d.filename
            FROM document_chunks AS c
            JOIN documents AS d ON d.id = c.document_id
            WHERE d.status = 'ready'
            """
        )
        keyword_ranked = self._keyword_rank(tokens, rows)
        semantic_ranked: list[dict[str, object]] = []
        graph_ranked: list[dict[str, object]] = []
        if self.embedding_enabled:
            try:
                query_vector = (await self.ollama.embed(self.embedding_model, [query]))[0]
                semantic_ranked = self._semantic_rank(query_vector, rows)
                if self.graph_store:
                    graph_ranked = await self.graph_store.search(query, query_vector, 20)
            except (InsufficientVram, OllamaUnavailable, ValueError, TypeError, IndexError):
                semantic_ranked = []
        candidates = self._fuse_rankings(
            keyword_ranked,
            semantic_ranked,
            graph_ranked,
            limit=max(20, limit * 4),
        )
        return self._local_rerank(query, tokens, candidates, limit)

    def _keyword_rank(
        self,
        tokens: list[str],
        rows: list[dict[str, object]],
    ) -> list[dict[str, object]]:
        normalized_query = " ".join(tokens)
        ranked: list[tuple[float, dict[str, object]]] = []
        for row in rows:
            haystack_tokens = self._search_tokens(f"{row['filename']} {row['content']}")
            haystack = " ".join(haystack_tokens)
            haystack_set = set(haystack_tokens)
            matched = sum(1 for token in tokens if token in haystack_set)
            if not matched:
                continue
            score = matched / len(tokens)
            if normalized_query and normalized_query in haystack:
                score += 0.75
            ranked.append(
                (
                    score,
                    {
                        "chunk_id": row["chunk_id"],
                        "document_id": row["document_id"],
                        "filename": row["filename"],
                        "chunk_index": row["chunk_index"],
                        "section_id": row["section_id"],
                        "section_title": row["section_title"],
                        "section_level": row["section_level"],
                        "page_number": row["page_number"],
                        "content": row["content"],
                        "score": round(score, 4),
                    },
                )
            )
        ranked.sort(
            key=lambda item: (
                -item[0],
                str(item[1]["filename"]),
                int(item[1]["chunk_index"]),
            )
        )
        return [item for _, item in ranked]

    def _semantic_rank(
        self,
        query_vector: list[float],
        rows: list[dict[str, object]],
    ) -> list[dict[str, object]]:
        ranked: list[tuple[float, dict[str, object]]] = []
        for row in rows:
            if row["embedding_model"] != self.embedding_model or not row["embedding_json"]:
                continue
            vector = json.loads(str(row["embedding_json"]))
            similarity = self._cosine_similarity(query_vector, vector)
            if similarity < 0.3:
                continue
            ranked.append(
                (
                    similarity,
                    {
                        "chunk_id": row["chunk_id"],
                        "document_id": row["document_id"],
                        "filename": row["filename"],
                        "chunk_index": row["chunk_index"],
                        "section_id": row["section_id"],
                        "section_title": row["section_title"],
                        "section_level": row["section_level"],
                        "page_number": row["page_number"],
                        "content": row["content"],
                        "score": round(similarity, 4),
                    },
                )
            )
        ranked.sort(key=lambda item: -item[0])
        return [item for _, item in ranked[:20]]

    @staticmethod
    def _fuse_rankings(
        *rankings: list[dict[str, object]],
        limit: int,
    ) -> list[dict[str, object]]:
        scores: dict[str, float] = {}
        records: dict[str, dict[str, object]] = {}
        active = [ranking for ranking in rankings if ranking]
        if not active:
            return []
        weight = 1.0 / len(active)
        for ranking in active:
            for rank, record in enumerate(ranking[:20], start=1):
                chunk_id = str(record["chunk_id"])
                scores[chunk_id] = scores.get(chunk_id, 0.0) + weight / (60 + rank)
                records[chunk_id] = record
        ordered = sorted(scores, key=lambda chunk_id: -scores[chunk_id])
        selected: list[dict[str, object]] = []
        for chunk_id in ordered[: max(1, min(limit, 20))]:
            selected.append({**records[chunk_id], "score": round(scores[chunk_id] * 100, 4)})
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

    def _local_rerank(
        self,
        query: str,
        tokens: list[str],
        candidates: list[dict[str, object]],
        limit: int,
    ) -> list[dict[str, object]]:
        """Deterministic in-process reranker over the fused candidate set."""
        query_phrase = " ".join(tokens)
        query_terms = set(tokens)
        ranked: list[tuple[float, dict[str, object]]] = []
        for candidate in candidates:
            searchable = " ".join(
                self._search_tokens(
                    f"{candidate.get('filename', '')} "
                    f"{candidate.get('section_title', '')} "
                    f"{candidate.get('content', '')}"
                )
            )
            candidate_terms = set(searchable.split())
            overlap = len(query_terms & candidate_terms) / max(1, len(query_terms))
            phrase_bonus = 0.35 if query_phrase and query_phrase in searchable else 0.0
            heading_bonus = 0.15 if any(
                token in self._search_tokens(str(candidate.get("section_title") or ""))
                for token in query_terms
            ) else 0.0
            base = float(candidate.get("score") or 0.0)
            rerank_score = base + overlap + phrase_bonus + heading_bonus
            ranked.append(
                (
                    rerank_score,
                    {**candidate, "rerank_score": round(rerank_score, 4)},
                )
            )
        ranked.sort(
            key=lambda item: (
                -item[0],
                str(item[1].get("filename") or ""),
                int(item[1].get("chunk_index") or 0),
            )
        )
        return [item for _, item in ranked[: max(1, min(limit, 20))]]

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
            if not meaningful and (extension == ".pdf" or extension in IMAGE_EXTENSIONS):
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
                self.index_text(document_id, normalized)
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

    @classmethod
    def _chunk_text(cls, text: str, size: int = 1400, overlap: int = 180) -> list[str]:
        return [
            str(record["content"])
            for record in cls._chunk_records(text, size=size, overlap=overlap)
        ]

    @staticmethod
    def _search_tokens(value: str) -> list[str]:
        return [
            token
            for token in re.findall(r"[^\W_]+", value.casefold(), flags=re.UNICODE)
            if len(token) > 1
        ]

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

    def _markitdown_converter(self) -> MarkItDown:
        model = self._active_vision_model()
        with self._markitdown_lock:
            if self._markitdown is not None and self._markitdown_model == model:
                return self._markitdown
            options: dict[str, object] = {"enable_plugins": True}
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
            self._markitdown_model = model
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
