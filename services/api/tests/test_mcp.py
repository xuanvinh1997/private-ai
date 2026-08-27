from __future__ import annotations

from pathlib import Path

import pytest
from conftest import FakeIndex

from private_ai_api.config import Settings
from private_ai_api.mcp_server import create_mcp_server


@pytest.mark.asyncio
async def test_mcp_tools_share_documents_and_memory(tmp_path: Path) -> None:
    server = create_mcp_server(
        Settings(
            data_dir=tmp_path,
            frontend_dist=tmp_path / "missing-web",
            embedding_enabled=False,
        ),
        FakeIndex(),  # type: ignore[arg-type]
    )
    tool_names = {tool.name for tool in await server.list_tools()}
    assert {
        "workspaces.list",
        "documents.list",
        "documents.search",
        "documents.ingest_text",
        "documents.delete",
        "graph.search",
        "graph.neighborhood",
        "graph.find_entity",
        "graph.answer",
        "memory.list",
        "memory.remember",
        "memory.search",
        "memory.update",
        "memory.forget",
        "models.list",
        "models.status",
        "models.capabilities",
        "models.select_default",
    } <= tool_names
    assert "models.delete" not in tool_names

    asr_status = await server.call_tool(
        "models.status",
        {"name": "nemotron-3.5-asr-streaming-0.6b"},
    )
    assert asr_status.structured_content["model_type"] == "asr"
    selected = await server.call_tool(
        "models.select_default",
        {"task": "asr", "name": "nemotron-3.5-asr-streaming-0.6b"},
    )
    assert selected.structured_content == {
        "task": "asr",
        "model": "nemotron-3.5-asr-streaming-0.6b",
    }

    ingested = await server.call_tool(
        "documents.ingest_text",
        {
            "filename": "mcp-notes.md",
            "content": "MCP local knowledge contains the violet-lantern marker.",
            "workspace_id": "personal",
        },
    )
    assert ingested.structured_content["status"] == "ready"
    searched = await server.call_tool(
        "documents.search",
        {"query": "violet-lantern", "workspace_id": "personal", "limit": 3},
    )
    assert searched.structured_content["result"][0]["filename"] == "mcp-notes.md"
    assert "violet-lantern" in searched.structured_content["result"][0]["content"]

    remembered = await server.call_tool(
        "memory.remember",
        {"content": "Ưu tiên câu trả lời ngắn", "memory_type": "preference"},
    )
    memory_id = remembered.structured_content["id"]
    memories = await server.call_tool("memory.list", {})
    assert memories.structured_content["result"][0]["id"] == memory_id
    updated = await server.call_tool(
        "memory.update",
        {"memory_id": memory_id, "content": "Ưu tiên câu trả lời rất ngắn"},
    )
    assert updated.structured_content["content"] == "Ưu tiên câu trả lời rất ngắn"
    forgotten = await server.call_tool(
        "memory.forget",
        {"memory_id": memory_id, "confirmed": True},
    )
    assert forgotten.structured_content["forgotten"] is True
