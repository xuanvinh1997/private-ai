"""Dense retrieval as its own MCP server.

Splitting the strategies apart is what lets a model *choose* a retriever: it reads this
server's instructions and this tool's description and decides whether meaning or wording
is what will find the passage. Keep both written for that reader.
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
from private_ai.rag.strategies.vector import VectorStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-vector"

INSTRUCTIONS = (
    "Semantic (vector) retrieval over one workspace's documents.\n\n"
    f"{VectorStrategy.description}\n\n"
    "Choose this server when the user paraphrased the idea instead of quoting the text. "
    "Choose rag.keyword instead when an exact name, code or quoted phrase must match, "
    "and rag.hybrid when you cannot tell which of the two decides the answer.\n\n"
    f"{UNTRUSTED_FRAMING}"
)

DESCRIPTION = (
    "Tìm theo ngữ nghĩa (vector) trong tài liệu của một workspace. "
    f"{VectorStrategy.description}\n\n"
    "Trả về các trích đoạn kèm filename, document_id, chunk_id, page và score.\n"
    f"{UNTRUSTED_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME, "Private AI vector search", INSTRUCTIONS, settings=app.settings
    )
    strategy = app.strategies.get(VectorStrategy.name)

    @server.tool(name="rag.vector.search", description=DESCRIPTION)
    async def vector_search(query: str, workspace_id: str, limit: int = 5) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        documents = await strategy.retrieve(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 20)),
        )
        return results_payload(query, VectorStrategy.name, documents)

    return server


def run() -> None:
    stdio_entry(create_server)
