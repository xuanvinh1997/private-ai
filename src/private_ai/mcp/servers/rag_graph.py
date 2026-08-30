"""The knowledge graph as its own MCP server.

Three tools rather than one, because a graph answers three different shapes of question:
which passages are relevant, what surrounds a known entity, and which entities the
documents even mention.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from mcp.server.mcpserver import MCPServer

from private_ai.mcp.common import (
    UNTRUSTED_FRAMING,
    build_server,
    require_workspace,
    resolve_services,
    results_payload,
    stdio_entry,
)
from private_ai.rag.strategies.graph import GraphStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-graph"

INSTRUCTIONS = (
    "Entity-graph retrieval over one workspace, built at ingestion time by LightRAG.\n\n"
    f"{GraphStrategy.description}\n\n"
    "Choose this server for multi-hop questions: who is connected to whom, how one "
    "concept leads to another, a chain of events spanning several documents. It only "
    "sees documents that were indexed in graph mode — when it comes back empty, fall "
    "back to rag.hybrid rather than concluding the library says nothing.\n\n"
    "rag.graph.search returns passages; rag.graph.entities tells you which entity names "
    "exist so you can spell one correctly; rag.graph.neighborhood expands one of them.\n\n"
    f"{UNTRUSTED_FRAMING}"
)

SEARCH_DESCRIPTION = (
    "Tìm trên đồ thị tri thức của một workspace. "
    f"{GraphStrategy.description}\n\n"
    "Dùng cho câu hỏi nhiều bước về quan hệ giữa các thực thể. Trả về các trích đoạn kèm "
    "filename, document_id, chunk_id, page và score. Danh sách rỗng nghĩa là workspace "
    "này chưa lập chỉ mục graph — hãy chuyển sang rag.hybrid.\n"
    f"{UNTRUSTED_FRAMING}"
)

ENTITIES_DESCRIPTION = (
    "Liệt kê các thực thể trong đồ thị của workspace khớp với truy vấn. Dùng trước "
    "rag.graph.neighborhood để lấy đúng entity_key, hoặc để biết tài liệu nói tới những "
    "ai/những gì. Không trả về đoạn văn bản.\n"
    f"{UNTRUSTED_FRAMING}"
)

NEIGHBORHOOD_DESCRIPTION = (
    "Mở rộng một thực thể đã biết ra các thực thể và quan hệ xung quanh nó trong cùng "
    "workspace. Dùng sau khi rag.graph.entities đã cho entity_key chính xác, khi cần biết "
    "một thực thể nối với những gì chứ không phải tài liệu nói gì về nó.\n"
    f"{UNTRUSTED_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME,
        "Private AI knowledge graph",
        INSTRUCTIONS,
        settings=app.settings,
    )
    strategy = app.strategies.get(GraphStrategy.name)

    @server.tool(name="rag.graph.search", description=SEARCH_DESCRIPTION)
    async def graph_search(
        query: str,
        workspace_id: str,
        limit: int = 5,
        mode: str = "mix",
    ) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        documents = await strategy.retrieve(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 20)),
            mode=mode or "mix",
        )
        return results_payload(query, GraphStrategy.name, documents)

    @server.tool(name="rag.graph.entities", description=ENTITIES_DESCRIPTION)
    async def graph_entities(
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        entities = await strategy.entities(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 100)),
        )
        return {"query": query, "strategy": GraphStrategy.name, "entities": entities}

    @server.tool(name="rag.graph.neighborhood", description=NEIGHBORHOOD_DESCRIPTION)
    async def graph_neighborhood(
        entity_key: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        found = await strategy.neighborhood(
            entity_key,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 100)),
        )
        return {"entity_key": entity_key, "strategy": GraphStrategy.name, **found}

    return server


def run() -> None:
    stdio_entry(create_server)
