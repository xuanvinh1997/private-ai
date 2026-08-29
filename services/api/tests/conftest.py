from __future__ import annotations

from collections.abc import Callable, Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from private_ai_api.config import Settings
from private_ai_api.main import create_app


class FakeIndex:
    """Stands in for LightRAG: keeps documents in memory and matches on words."""

    def __init__(self) -> None:
        self.documents: dict[tuple[str, str], dict[str, str]] = {}
        self.embedding_model = "test-embedding"
        self.embedding_batch_size = 32
        self.embedding_concurrency = 4
        self.last_search_mode = "mix"
        self.index_document_calls = 0
        self.last_graph_model = ""

    async def list_models(self) -> list[object]:
        return []

    async def embed(self, _model: str, inputs: list[str]) -> list[list[float]]:
        return [
            [float(len(value)), float(sum(ord(char) for char in value) % 997)]
            for value in inputs
        ]

    async def index_document(
        self,
        workspace_id: str,
        document_id: str,
        filename: str,
        text: str,
        on_progress: Callable[[dict[str, object]], None] | None = None,
        graph_model: str = "",
    ) -> bool:
        if not text.strip():
            return False
        self.index_document_calls += 1
        self.last_graph_model = graph_model
        if on_progress:
            on_progress(
                {
                    "step": "embedding",
                    "progress": 0.72,
                    "detail": "Đã tạo 1 vector embedding",
                    "embedded_vectors": 1,
                    "estimated_chunks": 1,
                    "vectors_per_second": 12.5,
                    "elapsed_seconds": 0.08,
                }
            )
        self.documents[(workspace_id, document_id)] = {"filename": filename, "text": text}
        return True

    async def delete_document(self, workspace_id: str, document_id: str) -> bool:
        return self.documents.pop((workspace_id, document_id), None) is not None

    async def search(
        self,
        query: str,
        workspace_id: str,
        limit: int = 5,
        *,
        mode: str = "mix",
    ) -> list[dict[str, object]]:
        self.last_search_mode = mode
        tokens = [token for token in query.casefold().split() if token]
        matched = [
            {"content": item["text"], "filename": item["filename"], "chunk_id": document_id}
            for (owner, document_id), item in self.documents.items()
            if owner == workspace_id and any(token in item["text"].casefold() for token in tokens)
        ]
        return matched[:limit]

    async def find_entities(
        self,
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, object]]:
        needle = query.casefold().strip()
        names = sorted(
            {
                word.strip(".,").capitalize()
                for (owner, _), item in self.documents.items()
                if owner == workspace_id
                for word in item["text"].split()
                if len(word) > 3
            }
        )
        return [{"name": name} for name in names if not needle or needle in name.casefold()][:limit]

    async def knowledge_graph(
        self,
        workspace_id: str,
        entity: str = "*",
        depth: int = 2,
        limit: int = 200,
    ) -> dict[str, object]:
        entities = await self.find_entities("" if entity == "*" else entity, workspace_id, limit)
        names = [str(item["name"]) for item in entities]
        return {
            "entity": entity,
            "nodes": [
                {"id": name, "labels": [name], "properties": {"entity_type": "word"}}
                for name in names
            ],
            "edges": [
                {"source": names[0], "target": name, "type": "related", "properties": {}}
                for name in names[1:]
            ],
            "truncated": False,
        }

    async def neighborhood(
        self,
        entity: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        return {"entity": entity, "nodes": [], "edges": []}

    async def use_embedding_model(self, name: str) -> None:
        self.embedding_model = name

    async def configure_indexing(self, *, batch_size: int, concurrency: int) -> None:
        self.embedding_batch_size = batch_size
        self.embedding_concurrency = concurrency

    async def health(self) -> bool:
        return True

    async def close(self) -> None:
        return None


def _client(settings: Settings) -> Iterator[TestClient]:
    with TestClient(create_app(settings)) as test_client:
        index = FakeIndex()
        test_client.app.state.services.lightrag = index
        test_client.app.state.services.document_processor.lightrag = index
        test_client.app.state.services.document_processor.ai = index
        yield test_client


@pytest.fixture
def client(tmp_path: Path) -> Iterator[TestClient]:
    yield from _client(
        Settings(
            data_dir=tmp_path,
            frontend_dist=tmp_path / "missing-web",
            embedding_enabled=False,
        )
    )


@pytest.fixture
def queue_only_client(tmp_path: Path) -> Iterator[TestClient]:
    """The API as the desktop build runs it: ingestion belongs to a separate process."""
    yield from _client(
        Settings(
            data_dir=tmp_path,
            frontend_dist=tmp_path / "missing-web",
            embedding_enabled=False,
            inline_ingestion=False,
        )
    )
