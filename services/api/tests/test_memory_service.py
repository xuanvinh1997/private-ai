from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest

from private_ai_api.database import Database
from private_ai_api.services.memory_service import MemoryService


class FakeOllama:
    async def embed(self, model: str, inputs: list[str]) -> list[list[float]]:
        assert model == "test-embedding"
        vectors = {
            "Prefers concise answers": [1.0, 0.0],
            "Lives in Hanoi": [0.0, 1.0],
            "short response": [1.0, 0.0],
        }
        return [vectors[value] for value in inputs]


class FakeGraph:
    def __init__(self) -> None:
        self.synced: list[str] = []
        self.deleted: list[str] = []

    async def sync_memory(self, memory_id: str) -> bool:
        self.synced.append(memory_id)
        return True

    async def search_memories(self, *args: Any, **kwargs: Any) -> list[dict[str, object]]:
        return []

    async def delete_memory(self, memory_id: str) -> bool:
        self.deleted.append(memory_id)
        return True


def insert_memory(database: Database, memory_id: str, content: str) -> None:
    now = datetime.now(UTC).isoformat()
    database.execute(
        """
        INSERT INTO memories(
            id, user_id, type, content, source, confidence, enabled,
            created_at, updated_at, expires_at
        ) VALUES (?, 'local-user', 'preference', ?, 'test', 1, 1, ?, ?, NULL)
        """,
        (memory_id, content, now, now),
    )


@pytest.mark.asyncio
async def test_memory_service_embeds_searches_and_deletes(tmp_path: Path) -> None:
    database = Database(tmp_path / "memory.db")
    database.initialize()
    insert_memory(database, "concise", "Prefers concise answers")
    insert_memory(database, "location", "Lives in Hanoi")
    service = MemoryService(
        database,
        FakeOllama(),  # type: ignore[arg-type]
        embedding_model="test-embedding",
        embedding_enabled=True,
    )

    await service.sync_all()
    results = await service.search("short response", limit=1)

    assert results[0]["id"] == "concise"
    assert "embedding_json" not in results[0]
    assert database.fetch_one(
        "SELECT embedding_model FROM memories WHERE id = 'concise'"
    )["embedding_model"] == "test-embedding"

    await service.delete_memory("concise")
    assert database.fetch_one("SELECT id FROM memories WHERE id = 'concise'") is None
