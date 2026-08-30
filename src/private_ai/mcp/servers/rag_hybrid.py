"""Dense and lexical retrieval fused by rank, as its own MCP server."""

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
from private_ai.rag.strategies.hybrid import HybridStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-hybrid"

INSTRUCTIONS = (
    "Hybrid retrieval over one workspace's documents: semantic and keyword search run "
    "together and their rankings are fused.\n\n"
    f"{HybridStrategy.description}\n\n"
    "This is the safe default for an ordinary question about the documents. Choose "
    "rag.vector or rag.keyword instead only when you can say which of the two decides "
    "the answer, rag.graph for a question about how entities relate, and rag.summary "
    "when the user asked for a whole document to be summarized.\n\n"
    f"{UNTRUSTED_FRAMING}"
)

DESCRIPTION = (
    "Tìm kết hợp ngữ nghĩa và từ khóa trong tài liệu của một workspace. "
    f"{HybridStrategy.description}\n\n"
    "Trả về các trích đoạn kèm filename, document_id, chunk_id, page và score.\n"
    f"{UNTRUSTED_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME,
        "Private AI hybrid search",
        INSTRUCTIONS,
        settings=app.settings,
    )
    strategy = app.strategies.get(HybridStrategy.name)

    @server.tool(name="rag.hybrid.search", description=DESCRIPTION)
    async def hybrid_search(query: str, workspace_id: str, limit: int = 5) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        documents = await strategy.retrieve(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 20)),
        )
        return results_payload(query, HybridStrategy.name, documents)

    return server


def run() -> None:
    stdio_entry(create_server)
