from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from private_ai_api.schemas import ChatRequest
from private_ai_api.services.lightrag_store import LightRagStore

DIMENSION = 8


class StubProvider:
    """Answers embed and chat without a model, so the wiring is what gets exercised."""

    def __init__(self) -> None:
        self.embed_calls = 0
        self.chat_calls = 0

    async def embed(self, model: str, inputs: list[str]) -> list[list[float]]:
        self.embed_calls += 1
        # One direction for everything: retrieval is then decided by wiring, not ranking.
        return [[1.0] + [0.0] * (DIMENSION - 1) for _ in inputs]

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        self.chat_calls += 1
        return {"message": {"role": "assistant", "content": ""}, "done": True}


def test_lightrag_indexes_scopes_and_deletes_in_process(tmp_path: Path) -> None:
    """One event loop for the whole scenario.

    LightRAG keeps its worker pool in process-global state bound to the loop that created
    it, which matches the long-lived loop the API runs on but not a loop per test.
    """

    async def scenario() -> dict[str, Any]:
        provider = StubProvider()
        store = LightRagStore(
            tmp_path,
            provider,  # type: ignore[arg-type]
            embedding_model="stub-embed",
            resolve_chat_model=lambda: "stub-chat",
        )
        try:
            indexed = await store.index_document(
                "personal",
                "doc-1",
                "ke-hoach.md",
                "Máy chủ Ollama chạy trong WSL2 và phục vụ mô hình cho Private AI.",
            )
            await store.index_document(
                "research",
                "doc-2",
                "khac.md",
                "Ghi chú của một không gian làm việc khác.",
            )
            return {
                "indexed": indexed,
                "embed_calls": provider.embed_calls,
                "hits": await store.search("Ollama WSL2", "personal", limit=3),
                "other_workspace": await store.search("Ollama WSL2", "research", limit=3),
                "after_delete": (
                    await store.delete_document("personal", "doc-1"),
                    await store.search("Ollama WSL2", "personal", limit=3),
                ),
            }
        finally:
            await store.close()

    result = asyncio.run(scenario())

    assert result["indexed"] is True
    assert result["embed_calls"] > 0

    hits = result["hits"]
    assert hits, "the freshly indexed document should be retrievable"
    assert hits[0]["filename"] == "ke-hoach.md"
    assert "WSL2" in str(hits[0]["content"])

    # A workspace only ever answers from its own documents. The stub embedding matches
    # everything, so anything leaking across namespaces would show up here.
    other = result["other_workspace"]
    assert [hit["filename"] for hit in other] == ["khac.md"]

    deleted, remaining = result["after_delete"]
    assert deleted is True
    assert remaining == []

    # Everything lives in files beside the database; no server was involved.
    assert (tmp_path / "lightrag").is_dir()


def test_knowledge_graph_reads_an_empty_workspace_without_failing(tmp_path: Path) -> None:
    """A space that has never been indexed answers with an empty graph, not an error."""

    async def scenario() -> dict[str, Any]:
        store = LightRagStore(
            tmp_path,
            StubProvider(),  # type: ignore[arg-type]
            embedding_model="stub-embed",
            resolve_chat_model=lambda: "stub-chat",
        )
        try:
            return {
                "all": await store.knowledge_graph("personal"),
                "entity": await store.knowledge_graph("personal", entity="Ollama", depth=2),
                "neighborhood": await store.neighborhood("Ollama", "personal"),
            }
        finally:
            await store.close()

    result = asyncio.run(scenario())

    assert result["all"] == {"entity": "*", "nodes": [], "edges": [], "truncated": False}
    assert result["entity"]["entity"] == "Ollama"
    assert result["entity"]["nodes"] == []
    assert result["neighborhood"] == {"entity": "Ollama", "nodes": [], "edges": []}
