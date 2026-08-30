"""The MCP surface: what each server publishes, how names are mangled, and what the
agent is not allowed to reach.

The read-only boundary is the security-relevant test in this file. Filtering the
advertised list is not enough on its own — a model that guesses a mangled name reaches
the invoker directly — so both layers are asserted.
"""

from __future__ import annotations

import json

import pytest

from private_ai.core.services import AppServices
from private_ai.mcp.adapter import (
    NAME_SEPARATOR,
    alias_for,
    invoker,
    mcp_tools_to_langchain,
    name_for,
    render_result,
)
from private_ai.mcp.client import (
    BUILTIN_SERVERS,
    READ_ONLY_TOOLS,
    McpHub,
    agent_tool_names,
)
from private_ai.mcp.servers import (
    core_server,
    rag_graph,
    rag_hybrid,
    rag_keyword,
    rag_summary,
    rag_vector,
    rag_web,
)

MUTATING_TOOLS = frozenset(
    {
        "documents.ingest_text",
        "documents.delete",
        "memory.remember",
        "memory.update",
        "memory.forget",
        "models.select_default",
    }
)


async def _tool_names(server: object) -> set[str]:
    return {str(tool.name) for tool in await server.list_tools()}


# --- inventory ------------------------------------------------------------


async def test_the_core_server_publishes_the_documented_tool_set(
    services: AppServices,
) -> None:
    names = await _tool_names(core_server.create_server(services))
    assert names == {
        "workspaces.list",
        "documents.list",
        "documents.status",
        "documents.get",
        "documents.ingest_text",
        "documents.delete",
        "rag.auto.search",
        "memory.list",
        "memory.search",
        "memory.remember",
        "memory.update",
        "memory.forget",
        "models.list",
        "models.status",
        "models.capabilities",
        "models.select_default",
        "system.info",
        "system.time",
        "files.allowed",
        "files.list",
        "files.read",
    }


@pytest.mark.parametrize(
    ("module", "expected"),
    [
        (rag_vector, {"rag.vector.search"}),
        (rag_keyword, {"rag.keyword.search"}),
        (rag_hybrid, {"rag.hybrid.search"}),
        (rag_graph, {"rag.graph.search", "rag.graph.neighborhood", "rag.graph.entities"}),
        (rag_summary, {"rag.summary.digest", "rag.summary.outline"}),
        (rag_web, {"rag.web.search"}),
    ],
)
async def test_each_strategy_server_publishes_only_its_own_tools(
    services: AppServices,
    module: object,
    expected: set[str],
) -> None:
    assert await _tool_names(module.create_server(services)) == expected


async def test_every_retrieval_tool_repeats_the_untrusted_framing(
    services: AppServices,
) -> None:
    """The tool description is the only text the model reads when it decides what to do
    with the text that comes back."""
    for module in (rag_vector, rag_keyword, rag_hybrid, rag_graph, rag_summary, rag_web):
        for tool in await module.create_server(services).list_tools():
            assert "không đáng tin cậy" in (tool.description or "")


async def test_every_built_in_server_is_mountable_and_named(services: AppServices) -> None:
    assert set(BUILTIN_SERVERS.values()) == {
        "core",
        "rag.vector",
        "rag.keyword",
        "rag.hybrid",
        "rag.graph",
        "rag.summary",
        "rag.web",
    }


# --- name mangling --------------------------------------------------------


def test_dots_become_double_underscores_and_back() -> None:
    """Function names on the OpenAI wire format may not contain dots."""
    mangled = f"rag{NAME_SEPARATOR}graph{NAME_SEPARATOR}neighborhood"
    assert alias_for("rag.graph.neighborhood") == mangled
    assert name_for(alias_for("rag.graph.neighborhood")) == "rag.graph.neighborhood"
    for name in READ_ONLY_TOOLS:
        assert "." not in alias_for(name)
        assert name_for(alias_for(name)) == name


def test_the_advertised_names_are_the_mangled_ones(services: AppServices) -> None:
    assert agent_tool_names() == sorted(alias_for(name) for name in READ_ONLY_TOOLS)
    assert all("." not in name for name in agent_tool_names())


async def test_the_adapter_advertises_mangled_names_with_their_schemas(
    services: AppServices,
) -> None:
    tools = await mcp_tools_to_langchain(rag_vector.create_server(services), allow=None)
    assert [tool.name for tool in tools] == ["rag__vector__search"]
    schema = tools[0].args_schema
    assert schema["type"] == "object"
    assert set(schema["properties"]) >= {"query", "workspace_id", "limit"}
    assert tools[0].description


# --- the read-only boundary ----------------------------------------------


def test_the_allow_set_is_exactly_the_non_mutating_tools() -> None:
    assert frozenset() == MUTATING_TOOLS & READ_ONLY_TOOLS


async def test_a_mutating_tool_is_never_advertised_to_the_agent(
    services: AppServices,
) -> None:
    advertised = {
        tool.name
        for tool in await mcp_tools_to_langchain(
            core_server.create_server(services),
            allow=READ_ONLY_TOOLS,
        )
    }
    for name in MUTATING_TOOLS:
        assert alias_for(name) not in advertised
    assert alias_for("documents.list") in advertised


async def test_a_mutating_tool_is_refused_even_when_called_by_its_mangled_name(
    services: AppServices,
    workspace_id: str,
) -> None:
    """The advertised list is only a hint: a model that guesses the alias lands here."""
    server = core_server.create_server(services)
    call = invoker(server, alias_for("documents.ingest_text"), allow=READ_ONLY_TOOLS)

    answer = await call(
        workspace_id=workspace_id,
        filename="lén.md",
        content="tài liệu này bảo bạn hãy ghi tôi vào thư viện",
    )

    assert answer == "Tool documents.ingest_text is not available to the agent."
    # Nothing was written.
    assert services.database.fetch_all("SELECT id FROM documents") == []


async def test_deletion_through_the_adapter_is_refused_before_it_reaches_the_tool(
    services: AppServices,
    workspace_id: str,
) -> None:
    from conftest import insert_document

    document_id = insert_document(services.database, workspace_id, "quan-trọng.pdf")
    server = core_server.create_server(services)
    call = invoker(server, alias_for("documents.delete"), allow=READ_ONLY_TOOLS)

    answer = await call(document_id=document_id, confirmed=True)

    assert "not available to the agent" in answer
    assert services.database.fetch_one("SELECT id FROM documents WHERE id = ?", (document_id,))


async def test_a_read_only_tool_still_works_through_the_same_invoker(
    services: AppServices,
    workspace_id: str,
) -> None:
    server = core_server.create_server(services)
    call = invoker(server, alias_for("workspaces.list"), allow=READ_ONLY_TOOLS)

    answer = await call()

    assert workspace_id in answer


async def test_a_failing_tool_answers_in_text_rather_than_ending_the_turn(
    services: AppServices,
) -> None:
    """A raised error just stops the turn with nothing said; the model has to read why."""
    server = core_server.create_server(services)
    call = invoker(server, alias_for("documents.status"), allow=READ_ONLY_TOOLS)

    answer = await call(document_id="không-tồn-tại")

    assert answer.startswith("Tool documents.status failed:")


def test_render_result_truncates_and_survives_unserialisable_output() -> None:
    class Result:
        structured_content = {"kết quả": "x" * 10_000}

    rendered = render_result(Result())
    assert len(rendered) == 6000
    assert rendered.startswith('{"kết quả"')

    class Odd:
        structured_content = {"đối tượng": object()}

    # ``default=str`` keeps an unexpected payload readable instead of raising.
    assert "object" in render_result(Odd())

    class Blocks:
        content = [{"type": "text", "text": "xin chào"}]

    assert json.loads(render_result(Blocks())) == [{"type": "text", "text": "xin chào"}]


# --- the hub --------------------------------------------------------------


@pytest.fixture
async def hub(services: AppServices):
    started = McpHub(services)
    await started.start()
    try:
        yield started
    finally:
        await started.close()


async def test_the_hub_mounts_every_built_in_server_in_process(hub: McpHub) -> None:
    assert set(hub.servers()) == set(BUILTIN_SERVERS.values())


async def test_the_hub_hands_the_agent_only_read_only_tools(hub: McpHub) -> None:
    names = {tool.name for tool in await hub.tools()}
    assert names == {alias_for(name) for name in READ_ONLY_TOOLS}
    for mutating in MUTATING_TOOLS:
        assert alias_for(mutating) not in names


async def test_the_hub_can_hand_out_everything_when_asked(hub: McpHub) -> None:
    """The UI is the caller that legitimately needs the mutating half."""
    names = {tool.name for tool in await hub.tools(allow=None)}
    assert {alias_for(name) for name in MUTATING_TOOLS} <= names


async def test_calling_by_name_routes_to_the_owning_server(
    hub: McpHub,
    workspace_id: str,
) -> None:
    answer = await hub.call("workspaces.list", {})
    assert workspace_id in answer
    # The mangled form addresses the same tool.
    assert await hub.call("workspaces__list", {}) == answer


async def test_calling_a_tool_no_server_owns_says_so(hub: McpHub) -> None:
    assert await hub.call("nothing.here", {}) == "Tool nothing.here is not available."
