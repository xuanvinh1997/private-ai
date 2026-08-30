"""Personal memory, ranked the same way a retrieval strategy is.

A memory is one short sentence the user asked us to keep — a preference, a fact, or an
episode. There are tens of them, not thousands, so there is no vector index here: every
enabled row is loaded and scored in Python, which is cheaper than maintaining a second
store and lets the two rankings disagree usefully.

Two rankings, fused. Keyword overlap answers "did they say this word", cosine similarity
answers "did they mean this thing", and reciprocal rank fusion combines them without
either score having to be calibrated against the other — only the positions matter. When
neither ranking produces anything (no shared tokens, no usable embeddings) the rows come
back in confidence order, because a memory the user wrote is worth showing even when
nothing about the question matched it.

Embedding failure is never fatal. The provider may be down, out of VRAM or not chosen
yet; memory degrades to keyword search rather than taking the chat turn down with it.
"""

from __future__ import annotations

import asyncio
import json
import math
import re
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any
from uuid import uuid4

from langchain_core.callbacks import AsyncCallbackManagerForRetrieverRun
from langchain_core.documents import Document
from langchain_core.retrievers import BaseRetriever
from pydantic import ConfigDict

from private_ai.llm import InsufficientVram, ProviderUnavailable

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.database import Database
    from private_ai.llm.router import ModelRouter

__all__ = ["MemoryStore"]

MEMORY_TYPES = frozenset({"preference", "fact", "episodic"})
# Below this the vector is agreeing with the query about nothing but language.
SEMANTIC_FLOOR = 0.3
# The classic RRF constant: large enough that a rank-1 hit in one ranking cannot on its
# own outvote agreement between both rankings further down.
RRF_K = 60
MAX_SEARCH_LIMIT = 20

# Anything the provider can throw when it cannot embed. None of them should reach the
# caller: the contract is that a failed embedding degrades ranking, not the turn.
_EMBEDDING_FAILURES = (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError)

_SELECT_COLUMNS = """
    SELECT id, user_id, type, content, source, confidence, enabled,
           created_at, updated_at, expires_at, embedding_json, embedding_model
    FROM memories
"""


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _tokens(value: str) -> list[str]:
    return re.findall(r"[^\W_]{2,}", value.casefold(), flags=re.UNICODE)


def _cosine_similarity(left: list[float], right: list[float]) -> float:
    if not left or len(left) != len(right):
        return -1.0
    dot = sum(a * b for a, b in zip(left, right, strict=True))
    left_norm = math.sqrt(sum(value * value for value in left))
    right_norm = math.sqrt(sum(value * value for value in right))
    if not left_norm or not right_norm:
        return -1.0
    return dot / (left_norm * right_norm)


def _document(row: dict[str, Any], score: float) -> Document:
    return Document(
        page_content=str(row["content"]),
        metadata={
            "memory_id": str(row["id"]),
            "type": str(row["type"]),
            "source": str(row["source"]),
            "confidence": float(row["confidence"]),
            "score": score,
        },
    )


class MemoryStore:
    """Memories in SQLite, with their own embeddings for semantic recall."""

    def __init__(
        self,
        database: Database,
        router: ModelRouter,
        *,
        embedding_model: str,
        enabled: bool = True,
    ) -> None:
        self.database = database
        self.router = router
        self.embedding_model = embedding_model
        self.enabled = enabled
        # One lock per memory, so two turns re-embedding different rows do not serialise
        # and two turns re-embedding the same row do not both pay for it.
        self._locks: dict[str, asyncio.Lock] = {}

    # --- embedding --------------------------------------------------------

    def active_model(self) -> str:
        """The embedding model rows are stamped with, resolved when it was left blank."""
        if self.embedding_model:
            return self.embedding_model
        return self.router.default_model("embedding") or self.router.settings.embedding_model

    async def _embed(self, texts: list[str]) -> list[list[float]]:
        embeddings = self.router.embeddings(self.active_model())
        return await embeddings.aembed_documents(texts)

    async def _embed_query(self, query: str) -> list[float]:
        embeddings = self.router.embeddings(self.active_model())
        return await embeddings.aembed_query(query)

    # --- synchronisation --------------------------------------------------

    async def sync_all(self) -> None:
        rows = await self.database.fetch_all_async(
            "SELECT id FROM memories ORDER BY updated_at",
        )
        for row in rows:
            await self.sync_memory(str(row["id"]))

    async def sync_memory(self, memory_id: str) -> bool:
        """Ensure this memory has an embedding from the current model. False if it does not.

        Re-embeds when the vector is missing or was produced by a different model — a
        vector from another model is not comparable to today's query vector, so keeping
        it would silently poison the semantic ranking.
        """
        lock = self._locks.setdefault(memory_id, asyncio.Lock())
        async with lock:
            row = await self.database.fetch_one_async(
                "SELECT id, content, embedding_json, embedding_model FROM memories WHERE id = ?",
                (memory_id,),
            )
            if not row:
                return False
            model = self.active_model()
            embedded = bool(row["embedding_json"])
            stale = not row["embedding_json"] or str(row["embedding_model"] or "") != model
            if not self.enabled or not stale:
                return embedded
            try:
                vector = (await self._embed([str(row["content"])]))[0]
            except _EMBEDDING_FAILURES:
                return False
            await self.database.execute_async(
                "UPDATE memories SET embedding_json = ?, embedding_model = ? WHERE id = ?",
                (json.dumps(vector, separators=(",", ":")), model, memory_id),
            )
            return True

    # --- reads ------------------------------------------------------------

    async def search(self, query: str, *, user_id: str, limit: int = 5) -> list[Document]:
        rows = await self.database.fetch_all_async(
            f"""{_SELECT_COLUMNS}
            WHERE user_id = ? AND enabled = 1
              AND (expires_at IS NULL OR expires_at > ?)
            ORDER BY confidence DESC, updated_at DESC
            """,  # noqa: S608 - _SELECT_COLUMNS is a module constant, not user input
            (user_id, _now()),
        )
        if not rows:
            return []
        bounded = max(1, min(limit, MAX_SEARCH_LIMIT))

        rankings: list[list[dict[str, Any]]] = []
        keyword = self._keyword_rank(query, rows)
        if keyword:
            rankings.append(keyword)
        semantic = await self._semantic_rank(query, rows)
        if semantic:
            rankings.append(semantic)
        if not rankings:
            # Nothing matched, but the user still wrote these down.
            return [_document(row, float(row["confidence"])) for row in rows[:bounded]]
        return self._fuse(rankings, rows, bounded)

    def as_retriever(self, *, user_id: str, limit: int = 5) -> BaseRetriever:
        return MemoryRetriever(store=self, user_id=user_id, limit=limit)

    # --- writes -----------------------------------------------------------

    async def remember(
        self,
        content: str,
        *,
        memory_type: str = "fact",
        source: str = "user",
        user_id: str,
    ) -> str:
        text = content.strip()
        if not text:
            raise ValueError("Nội dung ghi nhớ không được để trống")
        if memory_type not in MEMORY_TYPES:
            raise ValueError(f"Unsupported memory type: {memory_type}")
        memory_id = str(uuid4())
        now = _now()
        await self.database.execute_async(
            """
            INSERT INTO memories(
                id, user_id, type, content, source, confidence, enabled, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, 1.0, 1, ?, ?)
            """,
            (memory_id, user_id, memory_type, text, source, now, now),
        )
        await self.sync_memory(memory_id)
        return memory_id

    async def update(self, memory_id: str, content: str, enabled: bool = True) -> None:
        text = content.strip()
        if not text:
            raise ValueError("Nội dung ghi nhớ không được để trống")
        # Drop the vector rather than update it: the new text has a different meaning and
        # a stale vector would rank the memory for the old one until the next sync.
        await self.database.execute_async(
            """
            UPDATE memories
            SET content = ?, enabled = ?, updated_at = ?, embedding_json = NULL,
                embedding_model = NULL
            WHERE id = ?
            """,
            (text, int(enabled), _now(), memory_id),
        )
        await self.sync_memory(memory_id)

    async def forget(self, memory_id: str, *, confirmed: bool = False) -> None:
        if not confirmed:
            raise ValueError("Memory deletion requires confirmation")
        await self.database.execute_async("DELETE FROM memories WHERE id = ?", (memory_id,))
        self._locks.pop(memory_id, None)

    # --- ranking ----------------------------------------------------------

    @staticmethod
    def _keyword_rank(query: str, rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
        tokens = set(_tokens(query))
        if not tokens:
            return []
        ranked: list[tuple[float, dict[str, Any]]] = []
        for row in rows:
            overlap = len(tokens & set(_tokens(str(row["content"]))))
            if overlap:
                ranked.append((overlap / len(tokens), row))
        ranked.sort(key=lambda item: -item[0])
        return [row for _, row in ranked]

    async def _semantic_rank(
        self,
        query: str,
        rows: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        if not self.enabled or not query.strip():
            return []
        try:
            query_vector = await self._embed_query(query)
        except _EMBEDDING_FAILURES:
            return []
        if not query_vector:
            return []
        model = self.active_model()
        ranked: list[tuple[float, dict[str, Any]]] = []
        for row in rows:
            if str(row["embedding_model"] or "") != model or not row["embedding_json"]:
                continue
            try:
                vector = json.loads(str(row["embedding_json"]))
            except json.JSONDecodeError:
                continue
            similarity = _cosine_similarity(query_vector, vector)
            if similarity >= SEMANTIC_FLOOR:
                ranked.append((similarity, row))
        ranked.sort(key=lambda item: -item[0])
        return [row for _, row in ranked]

    @staticmethod
    def _fuse(
        rankings: list[list[dict[str, Any]]],
        canonical_rows: list[dict[str, Any]],
        limit: int,
    ) -> list[Document]:
        scores: dict[str, float] = {}
        canonical = {str(row["id"]): row for row in canonical_rows}
        for ranking in rankings:
            for position, record in enumerate(ranking, start=1):
                memory_id = str(record["id"])
                if memory_id in canonical:
                    scores[memory_id] = scores.get(memory_id, 0.0) + 1 / (RRF_K + position)
        ordered = sorted(scores, key=lambda memory_id: -scores[memory_id])[:limit]
        return [_document(canonical[memory_id], scores[memory_id]) for memory_id in ordered]


class MemoryRetriever(BaseRetriever):
    """The store behind the LangChain retriever interface, for use inside chains."""

    model_config = ConfigDict(arbitrary_types_allowed=True)

    store: MemoryStore
    user_id: str
    limit: int = 5

    async def _aget_relevant_documents(
        self,
        query: str,
        *,
        run_manager: AsyncCallbackManagerForRetrieverRun,
    ) -> list[Document]:
        return await self.store.search(query, user_id=self.user_id, limit=self.limit)

    def _get_relevant_documents(self, query: str, *, run_manager: Any) -> list[Document]:
        # Every caller in this app is async; a sync entry point would need its own event
        # loop and would deadlock inside the UI's.
        raise NotImplementedError("MemoryRetriever is async-only; use ainvoke")
