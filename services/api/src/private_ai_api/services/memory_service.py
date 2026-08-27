from __future__ import annotations

import asyncio
import json
import math
import re
from datetime import UTC, datetime
from typing import Any

from private_ai_api.database import Database
from private_ai_api.services.gpu_lease import InsufficientVram
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRouter


class MemoryService:
    """Keeps memories in SQLite, with their own embeddings for semantic recall."""

    def __init__(
        self,
        database: Database,
        ollama: ProviderRouter,
        *,
        embedding_model: str,
        embedding_enabled: bool,
    ) -> None:
        self.database = database
        self.ollama = ollama
        self.embedding_model = embedding_model
        self.embedding_enabled = embedding_enabled
        self._locks: dict[str, asyncio.Lock] = {}

    async def sync_all(self) -> None:
        for memory in self.database.fetch_all("SELECT id FROM memories ORDER BY updated_at"):
            await self.sync_memory(str(memory["id"]))

    async def sync_memory(self, memory_id: str) -> bool:
        lock = self._locks.setdefault(memory_id, asyncio.Lock())
        async with lock:
            memory = self.database.fetch_one(
                "SELECT id, content, embedding_json, embedding_model "
                "FROM memories WHERE id = ?",
                (memory_id,),
            )
            if not memory:
                return False
            embedded = bool(memory["embedding_json"])
            if self.embedding_enabled and (
                not memory["embedding_json"]
                or memory["embedding_model"] != self.embedding_model
            ):
                try:
                    vector = (await self.ollama.embed(
                        self.embedding_model,
                        [str(memory["content"])],
                    ))[0]
                except (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError):
                    embedded = False
                else:
                    self.database.execute(
                        "UPDATE memories SET embedding_json = ?, embedding_model = ? WHERE id = ?",
                        (
                            json.dumps(vector, separators=(",", ":")),
                            self.embedding_model,
                            memory_id,
                        ),
                    )
                    embedded = True
            return embedded

    async def delete_memory(self, memory_id: str) -> None:
        self.database.execute("DELETE FROM memories WHERE id = ?", (memory_id,))
        self._locks.pop(memory_id, None)

    async def search(
        self,
        query: str,
        *,
        user_id: str = "local-user",
        limit: int = 5,
    ) -> list[dict[str, Any]]:
        now = datetime.now(UTC).isoformat()
        rows = self.database.fetch_all(
            """
            SELECT id, user_id, type, content, source, confidence, enabled,
                   created_at, updated_at, expires_at, embedding_json, embedding_model
            FROM memories
            WHERE user_id = ? AND enabled = 1
              AND (expires_at IS NULL OR expires_at > ?)
            ORDER BY confidence DESC, updated_at DESC
            """,
            (user_id, now),
        )
        if not rows:
            return []
        bounded_limit = max(1, min(limit, 20))
        rankings: list[list[dict[str, Any]]] = []
        keyword = self._keyword_rank(query, rows)
        if keyword:
            rankings.append(keyword)
        if self.embedding_enabled and query.strip():
            try:
                query_vector = (await self.ollama.embed(
                    self.embedding_model,
                    [query],
                ))[0]
            except (InsufficientVram, ProviderUnavailable, IndexError, TypeError, ValueError):
                query_vector = []
            if query_vector:
                semantic = self._semantic_rank(query_vector, rows)
                if semantic:
                    rankings.append(semantic)
        if not rankings:
            return [self._public_record(row) for row in rows[:bounded_limit]]
        return self._fuse(rankings, rows, bounded_limit)

    @staticmethod
    def _tokens(value: str) -> list[str]:
        return re.findall(r"[^\W_]{2,}", value.casefold(), flags=re.UNICODE)

    @classmethod
    def _keyword_rank(
        cls,
        query: str,
        rows: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        tokens = set(cls._tokens(query))
        if not tokens:
            return []
        ranked: list[tuple[float, dict[str, Any]]] = []
        for row in rows:
            content_tokens = set(cls._tokens(str(row["content"])))
            overlap = len(tokens & content_tokens)
            if overlap:
                ranked.append((overlap / len(tokens), row))
        ranked.sort(key=lambda item: -item[0])
        return [row for _, row in ranked]

    def _semantic_rank(
        self,
        query_vector: list[float],
        rows: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        ranked: list[tuple[float, dict[str, Any]]] = []
        for row in rows:
            if row["embedding_model"] != self.embedding_model or not row["embedding_json"]:
                continue
            vector = json.loads(str(row["embedding_json"]))
            similarity = self._cosine_similarity(query_vector, vector)
            if similarity >= 0.3:
                ranked.append((similarity, row))
        ranked.sort(key=lambda item: -item[0])
        return [row for _, row in ranked]

    @classmethod
    def _fuse(
        cls,
        rankings: list[list[dict[str, Any]]],
        canonical_rows: list[dict[str, Any]],
        limit: int,
    ) -> list[dict[str, Any]]:
        scores: dict[str, float] = {}
        canonical = {str(row["id"]): row for row in canonical_rows}
        for ranking in rankings:
            for position, record in enumerate(ranking, start=1):
                memory_id = str(record["id"])
                if memory_id in canonical:
                    scores[memory_id] = scores.get(memory_id, 0.0) + 1 / (60 + position)
        ordered = sorted(scores, key=lambda memory_id: -scores[memory_id])[:limit]
        return [cls._public_record(canonical[memory_id]) for memory_id in ordered]

    @staticmethod
    def _public_record(row: dict[str, Any]) -> dict[str, Any]:
        return {
            key: value
            for key, value in row.items()
            if key not in {"embedding_json", "embedding_model", "score"}
        }

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
