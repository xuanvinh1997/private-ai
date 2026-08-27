from __future__ import annotations

from collections.abc import Iterator
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

    async def index_document(
        self,
        workspace_id: str,
        document_id: str,
        filename: str,
        text: str,
    ) -> bool:
        if not text.strip():
            return False
        self.documents[(workspace_id, document_id)] = {"filename": filename, "text": text}
        return True

    async def delete_document(self, workspace_id: str, document_id: str) -> bool:
        return self.documents.pop((workspace_id, document_id), None) is not None

    async def search(
        self,
        query: str,
        workspace_id: str,
        limit: int = 5,
    ) -> list[dict[str, object]]:
        tokens = [token for token in query.casefold().split() if token]
        matched = [
            {"content": item["text"], "filename": item["filename"], "chunk_id": document_id}
            for (owner, document_id), item in self.documents.items()
            if owner == workspace_id
            and any(token in item["text"].casefold() for token in tokens)
        ]
        return matched[:limit]

    async def find_entities(
        self,
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, object]]:
        return []

    async def neighborhood(
        self,
        entity: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        return {"entity": entity, "nodes": [], "edges": []}

    async def use_embedding_model(self, name: str) -> None:
        self.embedding_model = name

    async def health(self) -> bool:
        return True

    async def close(self) -> None:
        return None


@pytest.fixture
def client(tmp_path: Path) -> Iterator[TestClient]:
    settings = Settings(
        data_dir=tmp_path,
        frontend_dist=tmp_path / "missing-web",
        embedding_enabled=False,
    )
    with TestClient(create_app(settings)) as test_client:
        index = FakeIndex()
        test_client.app.state.services.lightrag = index
        test_client.app.state.services.document_processor.lightrag = index
        yield test_client
