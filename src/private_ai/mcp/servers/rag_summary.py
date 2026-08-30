"""Exhaustive whole-document summarization as its own MCP server.

Two tools: ``rag.summary.outline`` says what a digest would cost before any tokens are
spent, and ``rag.summary.digest`` actually reads every chunk in order and map-reduces it.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from mcp.server.mcpserver import MCPServer
from mcp.server.mcpserver.exceptions import ToolError

from private_ai.mcp.common import (
    UNTRUSTED_FRAMING,
    build_server,
    require_workspace,
    resolve_services,
    stdio_entry,
)
from private_ai.rag.strategies.summary import SummaryScopeError, SummaryStrategy

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-rag-summary"

INSTRUCTIONS = (
    "Exhaustive summarization of one named document in a workspace.\n\n"
    f"{SummaryStrategy.description}\n\n"
    "Choose this server only when the user asked for a whole document — or one named "
    "part or volume of it — to be summarized, listed in full, or retold. It reads every "
    "chunk in source order instead of taking a top-k slice, and it costs many model calls: "
    "for a question about a detail, use rag.hybrid instead.\n\n"
    "Call rag.summary.outline first to see how large the job is; call rag.summary.digest "
    "to run it.\n\n"
    f"{UNTRUSTED_FRAMING}"
)

OUTLINE_DESCRIPTION = (
    "Cho biết rag.summary.digest sẽ tóm tắt tài liệu nào và tốn bao nhiêu lượt gọi mô "
    "hình, mà không tiêu tốn token nào. Gọi trước digest để xác nhận đúng tài liệu người "
    "dùng muốn. Trả về null khi không xác định được tài liệu từ câu hỏi — khi đó hãy hỏi "
    "lại tên tệp thay vì đoán.\n"
    f"{UNTRUSTED_FRAMING}"
)

DIGEST_DESCRIPTION = (
    "Tóm tắt vét cạn toàn bộ một tài liệu được gọi tên. "
    f"{SummaryStrategy.description}\n\n"
    "Chỉ dùng khi người dùng yêu cầu tóm tắt/liệt kê đầy đủ cả tài liệu (hoặc một phần/"
    "tập cụ thể của nó). Rất đắt: hãy gọi rag.summary.outline trước. Với câu hỏi chi tiết, "
    "dùng rag.hybrid.\n"
    f"{UNTRUSTED_FRAMING}"
)


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    server = build_server(
        SERVER_NAME,
        "Private AI document digest",
        INSTRUCTIONS,
        settings=app.settings,
    )
    strategy = app.strategies.get(SummaryStrategy.name)

    @server.tool(name="rag.summary.outline", description=OUTLINE_DESCRIPTION)
    async def summary_outline(query: str, workspace_id: str) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        try:
            outline = await strategy.outline(query, workspace_id)
        except SummaryScopeError as exc:
            raise ToolError(str(exc)) from exc
        if outline is None:
            return {
                "query": query,
                "strategy": SummaryStrategy.name,
                "resolved": False,
                "detail": "Không xác định được tài liệu cần tóm tắt. Hãy nêu rõ tên tệp.",
            }
        return {
            "query": query,
            "strategy": SummaryStrategy.name,
            "resolved": True,
            **outline,
        }

    @server.tool(name="rag.summary.digest", description=DIGEST_DESCRIPTION)
    async def summary_digest(
        query: str,
        workspace_id: str,
        model: str = "",
    ) -> dict[str, Any]:
        await require_workspace(app.database, workspace_id)
        try:
            plan = await strategy.scope(query, workspace_id)
            if plan is None:
                raise ToolError("Không xác định được tài liệu cần tóm tắt. Hãy nêu rõ tên tệp.")
            summary = await strategy.digest(query, workspace_id, model=model, plan=plan)
        except SummaryScopeError as exc:
            raise ToolError(str(exc)) from exc
        return {
            "query": query,
            "strategy": SummaryStrategy.name,
            "document_id": plan.document_id,
            "filename": plan.filename,
            "source_label": plan.source_label,
            "chunk_count": len(plan.chunks),
            "summary": summary,
        }

    return server


def run() -> None:
    stdio_entry(create_server)
