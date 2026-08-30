"""Lexical retrieval as its own MCP server: the words the user typed, as they typed them."""

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
from private_ai.rag.strategies.keyword import KeywordStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-keyword"

INSTRUCTIONS = (
    "Keyword retrieval over one workspace's documents.\n\n"
    f"{KeywordStrategy.description}\n\n"
    "Choose this server when the question carries something that must match literally: a "
    "proper name, a document or article number, an identifier, a function name, a phrase "
    "in quotes. Choose rag.vector when the user paraphrased, and rag.hybrid when both "
    "wording and meaning could decide it.\n\n"
    f"{UNTRUSTED_FRAMING}"
)

DESCRIPTION = (
    "Tìm theo từ khóa trong tài liệu của một workspace. "
    f"{KeywordStrategy.description}\n\n"
    "Trả về các trích đoạn kèm filename, document_id, chunk_id, page và score.\n"
    f"{UNTRUSTED_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME,
        "Private AI keyword search",
        INSTRUCTIONS,
        settings=app.settings,
    )
    strategy = app.strategies.get(KeywordStrategy.name)

    @server.tool(name="rag.keyword.search", description=DESCRIPTION)
    async def keyword_search(query: str, workspace_id: str, limit: int = 5) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        documents = await strategy.retrieve(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 20)),
        )
        return results_payload(query, KeywordStrategy.name, documents)

    return server


def run() -> None:
    stdio_entry(create_server)
