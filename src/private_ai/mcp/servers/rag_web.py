"""Web search as its own MCP server — the only tool in Private AI that leaves the machine."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from mcp.server.mcpserver import MCPServer

from private_ai.mcp.common import build_server, resolve_services, results_payload, stdio_entry
from private_ai.rag.strategies.web import WEB_FRAMING, WebStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-web"

INSTRUCTIONS = (
    "Public web search through the backend the user configured (SearXNG, DuckDuckGo or "
    "OpenAI).\n\n"
    f"{WebStrategy.description}\n\n"
    "This is the only tool in Private AI that sends anything off this machine, so it "
    "stays off until the user picks a search host, and a query leaves the device when you "
    "call it. Choose it when the question needs current information, or when the "
    "workspace's own documents certainly cannot hold the answer — search the workspace "
    "first. When the backend is unavailable the tool returns a notice rather than "
    "failing: keep the local results and say the web was not reachable.\n\n"
    f"{WEB_FRAMING}"
)

DESCRIPTION = (
    "Tìm trên web qua backend người dùng đã cấu hình. "
    f"{WebStrategy.description}\n\n"
    "Đây là công cụ duy nhất gửi dữ liệu ra khỏi máy này. Trả về results kèm url/filename "
    "và score, cùng notice khi backend chưa bật hoặc không kết nối được.\n"
    f"{WEB_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME,
        "Private AI web search",
        INSTRUCTIONS,
        settings=app.settings,
    )
    strategy = app.strategies.get(WebStrategy.name)

    @server.tool(name="rag.web.search", description=DESCRIPTION)
    async def web_search(query: str, limit: int = 5) -> dict[str, Any]:
        outcome = await strategy.search(query, limit=max(1, min(limit, 10)))
        payload = results_payload(query, WebStrategy.name, outcome.documents)
        for entry, document in zip(payload["results"], outcome.documents, strict=True):
            entry["url"] = str(document.metadata.get("url") or "")
            entry["title"] = str(document.metadata.get("title") or "")
        payload["backend"] = outcome.backend
        payload["notice"] = outcome.notice
        payload["framing"] = outcome.framing
        if outcome.summary:
            payload["summary"] = outcome.summary
        return payload

    return server


def run() -> None:
    stdio_entry(create_server)
