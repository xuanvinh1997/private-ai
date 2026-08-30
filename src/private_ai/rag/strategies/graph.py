"""Entity-graph retrieval, over the LightRAG index built at ingestion time."""

from __future__ import annotations

from typing import Any

from langchain_core.documents import Document

from private_ai.core.schemas import RetrievalStrategyName
from private_ai.rag.strategies.base import Strategy, deduplicate, stamp


class GraphStrategy(Strategy):
    name = RetrievalStrategyName.GRAPH.value
    description = (
        "Tìm trên đồ thị tri thức của workspace. Phù hợp nhất với câu hỏi nhiều bước về "
        "quan hệ giữa các thực thể: ai liên quan tới ai, một khái niệm nối với khái niệm "
        "nào, chuỗi sự kiện đi qua nhiều tài liệu. Chỉ dùng được với tài liệu đã lập chỉ "
        "mục ở chế độ graph."
    )

    async def retrieve(
        self,
        query: str,
        *,
        workspace_id: str,
        limit: int = 5,
        **options: Any,
    ) -> list[Document]:
        text = query.strip()
        if not text:
            return []
        mode = str(options.get("mode") or "mix")
        documents = await self.services.graph.search(text, workspace_id, max(1, limit), mode=mode)
        return stamp(deduplicate(documents, limit), self.name)

    # The graph MCP server publishes these two alongside `rag.graph.search`; they answer
    # questions about the graph itself rather than returning passages, so they stay off
    # the Strategy interface and are reached through the concrete class.

    async def neighborhood(
        self,
        entity_key: str,
        *,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, Any]:
        return await self.services.graph.neighborhood(entity_key, workspace_id, limit)

    async def entities(
        self,
        query: str,
        *,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, Any]]:
        return await self.services.graph.find_entities(query, workspace_id, limit)
