"""A chat turn sees exactly one workspace.

Documents belong to exactly one workspace, and that boundary is a privacy property, not a
convenience. It broke because the agent's tools took ``workspace_id`` as an ordinary
model-supplied argument and ``workspaces.list`` handed out every id in the library: asked
to "tóm tắt báo cáo" inside a workspace holding one PDF, the model enumerated the other
workspaces and described three documents from them.

The binding now lives in the tool schema, where the model cannot reach it.
"""

from __future__ import annotations

import pytest
from conftest import insert_document

from private_ai.core.database import Database
from private_ai.core.services import AppServices
from private_ai.mcp.adapter import WORKSPACE_FIELD
from private_ai.mcp.client import WORKSPACE_DIRECTORY_TOOL, McpHub


@pytest.fixture
async def hub(services: AppServices) -> McpHub:
    hub = McpHub(services)
    await hub.start()
    return hub


@pytest.mark.asyncio
async def test_a_bound_turn_cannot_see_the_workspace_directory(
    hub: McpHub, workspace_id: str
) -> None:
    unbound = {tool.name for tool in await hub.tools()}
    bound = {tool.name for tool in await hub.tools(workspace_id=workspace_id)}

    directory = WORKSPACE_DIRECTORY_TOOL.replace(".", "__")
    assert directory in unbound, "the UI still needs this tool"
    assert directory not in bound, "a pinned turn must not enumerate other workspaces"


@pytest.mark.asyncio
async def test_workspace_id_disappears_from_the_advertised_schema(
    hub: McpHub, workspace_id: str
) -> None:
    """What the model cannot see, it cannot get wrong."""
    tools = {tool.name: tool for tool in await hub.tools(workspace_id=workspace_id)}
    scoped = tools["documents__list"]

    schema = scoped.args_schema
    properties = schema["properties"] if isinstance(schema, dict) else {}
    assert WORKSPACE_FIELD not in properties
    assert WORKSPACE_FIELD not in (schema.get("required") or [])

    unbound = {tool.name: tool for tool in await hub.tools()}["documents__list"]
    assert WORKSPACE_FIELD in unbound.args_schema["properties"]


@pytest.mark.asyncio
async def test_a_tool_call_is_forced_onto_the_bound_workspace(
    services: AppServices,
    database: Database,
    workspace_id: str,
    other_workspace_id: str,
    hub: McpHub,
) -> None:
    """Even a model that guesses another workspace's id gets its own workspace back."""
    insert_document(database, workspace_id, "cua-toi.md", text="nội dung")
    insert_document(database, other_workspace_id, "cua-nguoi-khac.md", text="bí mật")

    tools = {tool.name: tool for tool in await hub.tools(workspace_id=workspace_id)}
    listing = tools["documents__list"]

    # The model supplies the *other* workspace's id anyway.
    result = await listing.coroutine(workspace_id=other_workspace_id)

    assert "cua-toi.md" in result
    assert "cua-nguoi-khac.md" not in result, "a bound tool leaked another workspace"


@pytest.mark.asyncio
async def test_an_unbound_hub_still_serves_external_clients(
    services: AppServices,
    database: Database,
    workspace_id: str,
    other_workspace_id: str,
    hub: McpHub,
) -> None:
    """Binding is for the agent. A real MCP client still chooses its own workspace."""
    insert_document(database, other_workspace_id, "cua-nguoi-khac.md", text="bí mật")

    tools = {tool.name: tool for tool in await hub.tools()}
    result = await tools["documents__list"].coroutine(workspace_id=other_workspace_id)
    assert "cua-nguoi-khac.md" in result


@pytest.mark.asyncio
async def test_binding_does_not_touch_tools_without_a_workspace(
    hub: McpHub, workspace_id: str
) -> None:
    """system.time has no workspace, so nothing should be injected into its call."""
    tools = {tool.name: tool for tool in await hub.tools(workspace_id=workspace_id)}
    result = await tools["system__time"].coroutine()
    assert result, "a workspace-less tool must still run"


@pytest.mark.asyncio
async def test_the_read_only_boundary_survives_binding(hub: McpHub, workspace_id: str) -> None:
    names = {tool.name for tool in await hub.tools(workspace_id=workspace_id)}
    for mutating in (
        "documents__delete",
        "documents__ingest_text",
        "memory__remember",
        "memory__forget",
        "models__select_default",
    ):
        assert mutating not in names


@pytest.mark.asyncio
async def test_each_workspace_gets_its_own_compiled_graph(services: AppServices) -> None:
    """One shared graph would hand every turn the first workspace's bound tools."""
    from private_ai.agent.runner import AgentRunner

    runner = AgentRunner(services)
    first = await runner.graph("ws-one")
    second = await runner.graph("ws-two")
    assert first is not second
    assert await runner.graph("ws-one") is first

    runner.reset()
    assert await runner.graph("ws-one") is not first
