"""A LangChain vector store over the application's own ``document_chunks`` table.

There is no separate vector database: chunks, their text and their embeddings live in
the same SQLite file as everything else, which is what makes the whole library one
portable file the user can copy. Embeddings are stored as raw float32 so a workspace
scan is a single numpy matrix-vector product rather than a JSON parse per row.
"""

from __future__ import annotations

import asyncio
import json
import re
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any
from uuid import uuid4

import numpy as np
from langchain_core.documents import Document
from langchain_core.vectorstores import VectorStore

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Coroutine, Iterable

    from langchain_core.embeddings import Embeddings

    from private_ai.core.database import Database

__all__ = ["SqliteVectorStore", "pack_vector", "unpack_vector"]

# Word characters in any script, so Vietnamese diacritics survive tokenisation.
TOKEN_PATTERN = re.compile(r"[^\W_]+", re.UNICODE)
# A long question would otherwise dilute the overlap ratio until nothing scores.
MAX_QUERY_TOKENS = 32
DEFAULT_EMBED_BATCH = 32

_CHUNK_COLUMNS = """
    c.id AS chunk_id, c.content, c.chunk_index, c.section_title, c.page_number,
    d.id AS document_id, d.filename
"""
_VECTOR_COLUMNS = ", c.embedding_vector, c.embedding_json, c.embedding_model"
# Graph-indexed documents are answered by LightRAG, not by this table.
_CHUNK_SOURCE = """
    FROM document_chunks AS c
    JOIN documents AS d ON d.id = c.document_id
    WHERE d.workspace_id = ? AND d.status = 'ready' AND d.index_mode = 'simple'
    ORDER BY d.created_at DESC, c.chunk_index
"""


def pack_vector(vector: list[float]) -> bytes:
    """Store an embedding as raw float32.

    JSON text cost roughly six bytes per dimension and a parse on every single search;
    the packed form is four bytes and numpy reads it without copying.
    """
    return np.asarray(vector, dtype=np.float32).tobytes()


def unpack_vector(packed: object, legacy_json: object) -> np.ndarray | None:
    """Read either storage form, so indexes built before the change still rank."""
    if isinstance(packed, bytes | bytearray | memoryview):
        buffer = bytes(packed)
        if not buffer or len(buffer) % 4:
            return None
        return np.frombuffer(buffer, dtype=np.float32)
    if isinstance(legacy_json, str) and legacy_json:
        try:
            return np.asarray(json.loads(legacy_json), dtype=np.float32)
        except (ValueError, TypeError):
            return None
    return None


def search_tokens(value: str) -> list[str]:
    return [token for token in TOKEN_PATTERN.findall(value.casefold()) if len(token) > 1]


def _semantic_scores(
    rows: list[dict[str, Any]],
    query_vector: list[float],
    embedding_model: str,
) -> list[float]:
    """Cosine similarity for every row at once, as a single matrix-vector product.

    Rows whose embedding is missing, stale or a different width score -1, which is what
    the caller reads as "no semantic opinion".
    """
    scores = [-1.0] * len(rows)
    if not query_vector or not embedding_model:
        return scores
    query = np.asarray(query_vector, dtype=np.float32)
    query_norm = float(np.linalg.norm(query))
    if not query_norm:
        return scores
    usable: list[int] = []
    vectors: list[np.ndarray] = []
    for position, row in enumerate(rows):
        if row["embedding_model"] != embedding_model:
            continue
        vector = unpack_vector(row["embedding_vector"], row["embedding_json"])
        if vector is None or vector.shape != query.shape:
            continue
        usable.append(position)
        vectors.append(vector)
    if not usable:
        return scores
    matrix = np.vstack(vectors)
    norms = np.linalg.norm(matrix, axis=1)
    safe = norms.copy()
    safe[safe == 0.0] = 1.0
    similarity = (matrix @ query) / (safe * query_norm)
    similarity[norms == 0.0] = -1.0
    for position, value in zip(usable, similarity.tolist(), strict=True):
        scores[position] = float(value)
    return scores


def _run_sync(coroutine: Coroutine[Any, Any, Any]) -> Any:
    """Run an async method from LangChain's synchronous surface.

    The whole application shares one asyncio loop, so blocking on it from inside itself
    would deadlock. The sync methods therefore only work off-loop; every caller in this
    codebase has an ``await`` available and should use the async twin.
    """
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(coroutine)
    coroutine.close()
    raise RuntimeError(
        "SqliteVectorStore's synchronous API cannot run inside a running event loop; "
        "await the async method instead (aadd_documents / asimilarity_search_with_score)."
    )


class SqliteVectorStore(VectorStore):
    """Chunk storage and retrieval for one workspace.

    An instance is bound to a workspace so it can be handed straight to LangChain as a
    retriever; ``scoped`` makes the sibling for another workspace without rebuilding the
    embeddings client.
    """

    def __init__(
        self,
        database: Database,
        embeddings: Embeddings,
        *,
        workspace_id: str = "",
        embedding_model: str = "",
    ) -> None:
        self.database = database
        self._embeddings = embeddings
        self.workspace_id = workspace_id
        # Rows embedded with a different model are skipped rather than rescored: their
        # vectors live in a different space and comparing them produces confident noise.
        self.embedding_model = embedding_model

    @property
    def embeddings(self) -> Embeddings:
        return self._embeddings

    def scoped(self, workspace_id: str) -> SqliteVectorStore:
        return SqliteVectorStore(
            self.database,
            self._embeddings,
            workspace_id=workspace_id,
            embedding_model=self.embedding_model,
        )

    # --- writing ----------------------------------------------------------

    async def aadd_documents(self, documents: list[Document], **kwargs: Any) -> list[str]:
        texts = [document.page_content for document in documents]
        metadatas = [dict(document.metadata) for document in documents]
        ids = (
            kwargs.pop("ids", None)
            or [document.id for document in documents if document.id]
            or None
        )
        if ids is not None and len(ids) != len(documents):
            ids = None
        return await self.aadd_texts(texts, metadatas, ids=ids, **kwargs)

    async def aadd_texts(
        self,
        texts: Iterable[str],
        metadatas: list[dict[str, Any]] | None = None,
        *,
        ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[str]:
        """Embed and store chunks. ``document_id`` must be known, per text or in kwargs."""
        contents = list(texts)
        if not contents:
            return []
        records = list(metadatas or [{} for _ in contents])
        if len(records) != len(contents):
            raise ValueError("metadatas must line up with texts")
        fallback_document = str(kwargs.get("document_id") or "")
        document_ids = [str(record.get("document_id") or fallback_document) for record in records]
        if not all(document_ids):
            raise ValueError("Every chunk needs a document_id")
        model = str(kwargs.get("embedding_model") or self.embedding_model).strip()
        if not model:
            raise ValueError("Không có mô hình nhúng nào đang hoạt động")

        if kwargs.get("replace"):
            for document_id in dict.fromkeys(document_ids):
                await self.adelete_document(document_id)

        batch_size = max(1, int(kwargs.get("batch_size") or DEFAULT_EMBED_BATCH))
        vectors: list[list[float]] = []
        for offset in range(0, len(contents), batch_size):
            vectors.extend(
                await self._embeddings.aembed_documents(contents[offset : offset + batch_size])
            )
        if len(vectors) != len(contents):
            raise ValueError("Nhà cung cấp trả về số vector không khớp số đoạn")

        chunk_ids = list(ids) if ids else [str(uuid4()) for _ in contents]
        offsets = await asyncio.to_thread(self._next_chunk_indexes, dict.fromkeys(document_ids))
        rows: list[tuple[Any, ...]] = []
        created_at = datetime.now(UTC).isoformat()
        for position, content in enumerate(contents):
            record = records[position]
            document_id = document_ids[position]
            index = record.get("chunk_index")
            if index is None:
                index = offsets[document_id]
                offsets[document_id] += 1
            rows.append(
                (
                    chunk_ids[position],
                    document_id,
                    int(index),
                    content,
                    record.get("section_id"),
                    record.get("section_title"),
                    int(record.get("section_level") or 0),
                    record.get("page")
                    if record.get("page") is not None
                    else record.get("page_number"),
                    pack_vector(vectors[position]),
                    model,
                    record.get("graph_model"),
                    created_at,
                )
            )
        await asyncio.to_thread(self._insert_rows, rows)
        return chunk_ids

    def _next_chunk_indexes(self, document_ids: Iterable[str]) -> dict[str, int]:
        offsets: dict[str, int] = {}
        for document_id in document_ids:
            row = self.database.fetch_one(
                "SELECT MAX(chunk_index) AS last FROM document_chunks WHERE document_id = ?",
                (document_id,),
            )
            last = row["last"] if row and row["last"] is not None else -1
            offsets[document_id] = int(last) + 1
        return offsets

    def _insert_rows(self, rows: list[tuple[Any, ...]]) -> None:
        self.database.execute_many(
            """
            INSERT INTO document_chunks(
                id, document_id, chunk_index, content, section_id, section_title,
                section_level, page_number, embedding_vector, embedding_model,
                graph_model, created_at, embedding_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
            ON CONFLICT(document_id, chunk_index) DO UPDATE SET
                content=excluded.content,
                section_id=excluded.section_id,
                section_title=excluded.section_title,
                section_level=excluded.section_level,
                page_number=excluded.page_number,
                embedding_vector=excluded.embedding_vector,
                embedding_model=excluded.embedding_model,
                embedding_json=NULL
            """,
            rows,
        )

    def add_texts(
        self,
        texts: Iterable[str],
        metadatas: list[dict[str, Any]] | None = None,
        *,
        ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[str]:
        return _run_sync(self.aadd_texts(texts, metadatas, ids=ids, **kwargs))

    async def adelete_document(self, document_id: str) -> None:
        await self.database.execute_async(
            "DELETE FROM document_chunks WHERE document_id = ?",
            (document_id,),
        )

    async def adelete(self, ids: list[str] | None = None, **kwargs: Any) -> bool | None:
        """Delete individual chunks; ``document_id`` in kwargs drops a whole document."""
        document_id = str(kwargs.get("document_id") or "")
        if document_id:
            await self.adelete_document(document_id)
            return True
        if not ids:
            return None
        await asyncio.to_thread(
            self.database.execute_many,
            "DELETE FROM document_chunks WHERE id = ?",
            [(chunk_id,) for chunk_id in ids],
        )
        return True

    def delete(self, ids: list[str] | None = None, **kwargs: Any) -> bool | None:
        return _run_sync(self.adelete(ids, **kwargs))

    async def acount(self, *, workspace_id: str = "") -> int:
        scope = workspace_id or self.workspace_id
        if scope:
            row = await self.database.fetch_one_async(
                "SELECT COUNT(*) AS total FROM document_chunks AS c "
                "JOIN documents AS d ON d.id = c.document_id WHERE d.workspace_id = ?",
                (scope,),
            )
        else:
            row = await self.database.fetch_one_async(
                "SELECT COUNT(*) AS total FROM document_chunks"
            )
        return int(row["total"]) if row else 0

    # --- reading ----------------------------------------------------------

    async def asimilarity_search_with_score(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> list[tuple[Document, float]]:
        workspace_id = str(kwargs.get("workspace_id") or self.workspace_id)
        model = str(kwargs.get("embedding_model") or self.embedding_model).strip()
        strategy = str(kwargs.get("strategy") or "vector")
        if not workspace_id or not query.strip() or not model:
            return []
        rows = await self._fetch_rows(workspace_id, with_vectors=True)
        if not rows:
            return []
        try:
            query_vector = await self._embeddings.aembed_query(query)
        except Exception:  # a provider outage must not break the answer path
            return []
        # Scoring touches every chunk in the workspace and nothing in it awaits, so it
        # runs in a thread rather than stalling the loop for the length of the scan.
        scores = await asyncio.to_thread(_semantic_scores, rows, list(query_vector), model)
        floor = float(kwargs.get("score_threshold", 0.0))
        ranked = sorted(
            ((score, row) for score, row in zip(scores, rows, strict=True) if score >= floor),
            key=lambda item: -item[0],
        )
        return [(_to_document(row, score, strategy), score) for score, row in ranked[: max(1, k)]]

    async def akeyword_search(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> list[tuple[Document, float]]:
        """Token overlap between the question and the chunk, filename included."""
        workspace_id = str(kwargs.get("workspace_id") or self.workspace_id)
        strategy = str(kwargs.get("strategy") or "keyword")
        tokens = list(dict.fromkeys(search_tokens(query)))[:MAX_QUERY_TOKENS]
        if not workspace_id or not tokens:
            return []
        rows = await self._fetch_rows(workspace_id, with_vectors=False)
        if not rows:
            return []
        ranked = await asyncio.to_thread(_rank_by_overlap, rows, tokens)
        return [(_to_document(row, score, strategy), score) for score, row in ranked[: max(1, k)]]

    async def asimilarity_search(self, query: str, k: int = 4, **kwargs: Any) -> list[Document]:
        return [
            document for document, _ in await self.asimilarity_search_with_score(query, k, **kwargs)
        ]

    def similarity_search(self, query: str, k: int = 4, **kwargs: Any) -> list[Document]:
        return _run_sync(self.asimilarity_search(query, k, **kwargs))

    def similarity_search_with_score(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> list[tuple[Document, float]]:
        return _run_sync(self.asimilarity_search_with_score(query, k, **kwargs))

    async def _fetch_rows(self, workspace_id: str, *, with_vectors: bool) -> list[dict[str, Any]]:
        columns = _CHUNK_COLUMNS + (_VECTOR_COLUMNS if with_vectors else "")
        return await self.database.fetch_all_async(
            f"SELECT {columns} {_CHUNK_SOURCE}",  # noqa: S608 - both fragments are literals
            (workspace_id,),
        )

    @classmethod
    def from_texts(
        cls,
        texts: list[str],
        embedding: Embeddings,
        metadatas: list[dict[str, Any]] | None = None,
        *,
        ids: list[str] | None = None,
        **kwargs: Any,
    ) -> SqliteVectorStore:
        """LangChain's constructor-plus-write helper. ``database`` is required."""
        database = kwargs.pop("database", None)
        if database is None:
            raise ValueError("SqliteVectorStore.from_texts needs the application Database")
        store = cls(
            database,
            embedding,
            workspace_id=str(kwargs.pop("workspace_id", "")),
            embedding_model=str(kwargs.pop("embedding_model", "")),
        )
        store.add_texts(texts, metadatas, ids=ids, **kwargs)
        return store


def _page_of(record: dict[str, Any]) -> int | None:
    """Chunk metadata calls it ``page``; the column is ``page_number``."""
    value = record.get("page")
    if value is None:
        value = record.get("page_number")
    return int(value) if value is not None else None


def _rank_by_overlap(
    rows: list[dict[str, Any]],
    tokens: list[str],
) -> list[tuple[float, dict[str, Any]]]:
    token_set = set(tokens)
    ranked: list[tuple[float, dict[str, Any]]] = []
    for row in rows:
        searchable = set(search_tokens(f"{row['filename']} {row['content']}"))
        matched = len(token_set & searchable)
        if not matched:
            continue
        ranked.append((matched / len(tokens), row))
    ranked.sort(key=lambda item: -item[0])
    return ranked


def _to_document(row: dict[str, Any], score: float, strategy: str) -> Document:
    metadata: dict[str, Any] = {
        "document_id": str(row["document_id"]),
        "filename": str(row["filename"]),
        "chunk_id": str(row["chunk_id"]),
        "score": round(float(score), 4),
        "strategy": strategy,
        "chunk_index": int(row["chunk_index"]),
    }
    if row.get("page_number") is not None:
        metadata["page"] = int(row["page_number"])
    if row.get("section_title"):
        metadata["section_title"] = str(row["section_title"])
    return Document(id=str(row["chunk_id"]), page_content=str(row["content"]), metadata=metadata)
