"""One whole chat turn through the LangGraph agent, against a scripted model."""

from __future__ import annotations

import asyncio
import json
from typing import Any

import pytest
from conftest import ScriptedChatModel, insert_document, scripted_model
from langchain_core.messages import AIMessage
from langchain_core.tools import StructuredTool

from private_ai.agent.graph import build_agent_graph, tool_rounds
from private_ai.agent.runner import EMPTY_ANSWER, NO_PROVIDER, AgentRunner
from private_ai.core import repositories
from private_ai.core.services import AppServices
from private_ai.llm import NoProviderConfigured
from private_ai.rag.web_search import WebSearchUnavailable

TOOL_ALIAS = "rag__vector__search"


@pytest.fixture
def tool_calls() -> list[dict[str, Any]]:
    return []


@pytest.fixture
def search_tool(tool_calls: list[dict[str, Any]]) -> StructuredTool:
    async def run(query: str) -> str:
        tool_calls.append({"query": query})
        return json.dumps(
            {"results": [{"filename": "xe.txt", "content": "chiếc xe màu đỏ"}]},
            ensure_ascii=False,
        )

    return StructuredTool(
        name=TOOL_ALIAS,
        description="Tìm theo ngữ nghĩa trong tài liệu của workspace.",
        args_schema={
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"],
        },
        coroutine=run,
        func=None,
    )


def install_model(
    services: AppServices,
    messages: list[AIMessage | str],
) -> tuple[ScriptedChatModel, list[list[str] | None]]:
    """Replace the router's chat model and record what each round was offered."""
    model = scripted_model(messages)
    offered: list[list[str] | None] = []

    def chat_model(
        name: str = "",
        *,
        streaming: bool = True,
        tools: list[Any] | None = None,
        **kwargs: Any,
    ) -> ScriptedChatModel:
        offered.append([tool.name for tool in tools] if tools else None)
        return model.bind_tools(tools) if tools else model

    services.models.chat_model = chat_model  # type: ignore[method-assign]
    return model, offered


@pytest.fixture
async def conversation(services: AppServices, workspace_id: str) -> str:
    record = await repositories.create_conversation(services.database, workspace_id)
    return record.id


async def runner_for(
    services: AppServices,
    tools: list[StructuredTool] | None = None,
) -> AgentRunner:
    runner = AgentRunner(services)
    runner._graph = await build_agent_graph(services, tools=tools or [])
    return runner


def _stream_kwargs(conversation: str, workspace_id: str, **overrides: Any) -> dict[str, Any]:
    return {
        "conversation_id": conversation,
        "content": "chiếc xe hơi màu gì?",
        "workspace_id": workspace_id,
        **overrides,
    }


# --- the budget -----------------------------------------------------------


def test_the_tool_round_budget_reserves_a_step_for_the_answer(
    services: AppServices,
) -> None:
    """A tool call plus the turn that reads its result costs two steps of the budget."""
    services.settings.agent_max_iterations = 3
    assert tool_rounds(services.settings) == 1
    services.settings.agent_max_iterations = 10
    assert tool_rounds(services.settings) == 4
    services.settings.agent_max_iterations = 1
    assert tool_rounds(services.settings) == 1


# --- a full turn ----------------------------------------------------------


async def test_a_turn_with_one_tool_round_emits_the_contracted_events(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    search_tool: StructuredTool,
    tool_calls: list[dict[str, Any]],
) -> None:
    model, offered = install_model(
        services,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": TOOL_ALIAS, "args": {"query": "xe hơi"}, "id": "call-1"}],
            ),
            AIMessage(content="Chiếc xe màu đỏ."),
        ],
    )
    runner = await runner_for(services, [search_tool])

    events = [event async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))]

    kinds = [event["type"] for event in events]
    assert kinds[0] == "tool_start"
    assert kinds[1] == "tool_end"
    assert "token" in kinds
    assert kinds[-1] == "final"

    start = events[0]
    assert start["name"] == TOOL_ALIAS
    assert start["args"] == {"query": "xe hơi"}
    assert tool_calls == [{"query": "xe hơi"}]
    assert "chiếc xe màu đỏ" in events[1]["output"]

    tokens = "".join(event["content"] for event in events if event["type"] == "token")
    assert tokens.strip() == "Chiếc xe màu đỏ."
    assert events[-1]["content"] == "Chiếc xe màu đỏ."

    # The last round is offered no tools, which is what forces an answer.
    assert offered == [[TOOL_ALIAS], None]
    assert model.bound_tools == [[TOOL_ALIAS]]


async def test_the_answer_and_the_run_are_written_down(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    search_tool: StructuredTool,
) -> None:
    install_model(
        services,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": TOOL_ALIAS, "args": {"query": "xe"}, "id": "c1"}],
            ),
            AIMessage(content="Màu đỏ."),
        ],
    )
    runner = await runner_for(services, [search_tool])

    async for _ in runner.stream(**_stream_kwargs(conversation, workspace_id)):
        pass

    detail = await repositories.get_conversation(services.database, conversation)
    assert [(m.role, m.content) for m in detail.messages] == [
        ("user", "chiếc xe hơi màu gì?"),
        ("assistant", "Màu đỏ."),
    ]
    # An untitled conversation is named after the question that started it.
    assert detail.title == "chiếc xe hơi màu gì?"

    run = services.database.fetch_one("SELECT * FROM agent_runs")
    assert run["status"] == "completed"
    assert run["error"] is None
    assert run["finished_at"]
    assert json.loads(str(run["tools_json"])) == [TOOL_ALIAS]


async def test_a_question_that_needs_no_tool_costs_no_round(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    search_tool: StructuredTool,
) -> None:
    _, offered = install_model(services, [AIMessage(content="Không cần công cụ.")])
    runner = await runner_for(services, [search_tool])

    events = [event async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))]

    assert [event["type"] for event in events if event["type"].startswith("tool")] == []
    assert offered == [[TOOL_ALIAS]]
    assert events[-1]["content"] == "Không cần công cụ."


async def test_a_model_that_keeps_asking_for_tools_is_stopped_not_looped(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    search_tool: StructuredTool,
) -> None:
    """Without the round cap this runs until the recursion limit turns it into an error."""
    call = [{"name": TOOL_ALIAS, "args": {"query": "lại nữa"}, "id": "c"}]
    install_model(
        services,
        [AIMessage(content="", tool_calls=call) for _ in range(5)],
    )
    runner = await runner_for(services, [search_tool])

    events = [event async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))]

    # Two model turns, then the graph ends because the tool-free round has been taken.
    assert [event["type"] for event in events].count("tool_start") == 2
    assert events[-1] == {"type": "error", "message": EMPTY_ANSWER}
    assert services.database.fetch_one("SELECT status FROM agent_runs")["status"] == "failed"


# --- citations and notices ------------------------------------------------


async def test_retrieved_documents_come_back_as_citations(
    services: AppServices,
    workspace_id: str,
    conversation: str,
) -> None:
    document_id = insert_document(services.database, workspace_id, "xe.txt")
    await services.vectors.scoped(workspace_id).aadd_texts(
        ["chiếc xe hơi màu đỏ đậu ngoài sân"],
        [{"document_id": document_id, "page": 2}],
    )
    install_model(services, [AIMessage(content="Màu đỏ [xe.txt].")])
    runner = await runner_for(services)

    final = [
        event
        async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))
        if event["type"] == "final"
    ][0]

    assert final["citations"]
    citation = final["citations"][0]
    assert citation["document_id"] == document_id
    assert citation["filename"] == "xe.txt"
    assert citation["page"] == 2
    assert citation["snippet"]
    assert citation["strategy"]


async def test_a_failed_web_search_is_a_notice_not_a_failed_turn(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    async def unavailable(*args: object, **kwargs: object) -> object:
        raise WebSearchUnavailable("Không kết nối được DuckDuckGo")

    monkeypatch.setattr(services.web_search, "search", unavailable)
    install_model(services, [AIMessage(content="Tôi trả lời bằng tài liệu cục bộ.")])
    runner = await runner_for(services)

    events = [
        event
        async for event in runner.stream(
            **_stream_kwargs(conversation, workspace_id, web_search=True)
        )
    ]

    notices = [event["message"] for event in events if event["type"] == "notice"]
    assert any("DuckDuckGo" in notice for notice in notices)
    assert events[-1]["type"] == "final"


# --- failure --------------------------------------------------------------


async def test_a_missing_provider_becomes_a_readable_error(
    services: AppServices,
    workspace_id: str,
    conversation: str,
) -> None:
    def unconfigured(*args: object, **kwargs: object) -> object:
        raise NoProviderConfigured("Chưa cấu hình nhà cung cấp AI nào")

    services.models.chat_model = unconfigured  # type: ignore[method-assign]
    runner = await runner_for(services)

    events = [event async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))]

    assert events[-1] == {"type": "error", "message": NO_PROVIDER}
    run = services.database.fetch_one("SELECT status, error FROM agent_runs")
    assert run["status"] == "failed"
    assert run["error"]
    # The user's question is still on record even though the answer never came.
    detail = await repositories.get_conversation(services.database, conversation)
    assert [message.role for message in detail.messages] == ["user"]


# --- cancellation ---------------------------------------------------------


async def test_cancelling_mid_stream_saves_the_partial_answer(
    services: AppServices,
    workspace_id: str,
    conversation: str,
) -> None:
    """Whatever the model had already said is written down in a synchronous ``finally``;
    awaiting there would be cancelled too, losing the very thing being saved."""
    install_model(services, [AIMessage(content="một hai ba bốn năm")])
    runner = await runner_for(services)
    stream = runner.stream(**_stream_kwargs(conversation, workspace_id))

    tokens: list[str] = []
    async for event in stream:
        if event["type"] == "token":
            tokens.append(event["content"])
        if len(tokens) >= 3:
            break

    with pytest.raises(asyncio.CancelledError):
        await stream.athrow(asyncio.CancelledError())
    # Let the graph's own tasks finish unwinding before the assertions read the database.
    await asyncio.sleep(0.05)

    detail = await repositories.get_conversation(services.database, conversation)
    saved = [message.content for message in detail.messages if message.role == "assistant"]
    assert saved == ["".join(tokens).strip()]
    assert saved[0] != "một hai ba bốn năm"
    assert "một hai ba bốn năm".startswith(saved[0])

    run = services.database.fetch_one("SELECT status FROM agent_runs")
    assert run["status"] == "cancelled"


# --- the collected form ---------------------------------------------------


async def test_run_collects_the_same_turn_without_streaming(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    async def unavailable(*args: object, **kwargs: object) -> object:
        raise WebSearchUnavailable("Không có backend tìm kiếm")

    monkeypatch.setattr(services.web_search, "search", unavailable)
    install_model(services, [AIMessage(content="Xong.")])
    runner = await runner_for(services)

    result = await runner.run(**_stream_kwargs(conversation, workspace_id, web_search=True))

    assert result["content"] == "Xong."
    assert result["error"] == ""
    assert result["notices"]
    assert isinstance(result["citations"], list)


# --- planning -------------------------------------------------------------


async def test_an_unknown_strategy_falls_back_to_auto(
    services: AppServices,
    workspace_id: str,
    conversation: str,
) -> None:
    install_model(services, [AIMessage(content="Xong.")])
    runner = await runner_for(services)

    async for _ in runner.stream(
        **_stream_kwargs(conversation, workspace_id, strategy="telepathy")
    ):
        pass

    assert services.database.fetch_one("SELECT strategy FROM agent_runs")["strategy"] == "auto"


async def test_explicitly_named_skills_are_the_ones_activated(
    services: AppServices,
    workspace_id: str,
    conversation: str,
) -> None:
    services.skills.refresh()
    install_model(services, [AIMessage(content="Xong.")])
    runner = await runner_for(services)

    async for _ in runner.stream(
        **_stream_kwargs(
            conversation,
            workspace_id,
            content="một câu hỏi không khớp kỹ năng nào",
            skills=["nghien-cuu-web"],
        )
    ):
        pass

    run = services.database.fetch_one("SELECT skills_json FROM agent_runs")
    assert json.loads(str(run["skills_json"])) == ["nghien-cuu-web"]


def test_the_recursion_limit_leaves_room_for_plan_and_retrieve(
    services: AppServices,
) -> None:
    from private_ai.agent.graph import agent_config

    services.settings.agent_max_iterations = 10
    assert agent_config(services.settings)["recursion_limit"] == 13


async def test_an_odd_iteration_budget_can_use_all_of_its_tool_rounds(
    services: AppServices,
    workspace_id: str,
    conversation: str,
    search_tool: StructuredTool,
) -> None:
    services.settings.agent_max_iterations = 5  # tool_rounds() == 2
    install_model(
        services,
        [
            AIMessage(
                content="",
                tool_calls=[{"name": TOOL_ALIAS, "args": {"query": "x"}, "id": f"c{index}"}],
            )
            for index in range(2)
        ]
        + [AIMessage(content="Xong.")],
    )
    runner = await runner_for(services, [search_tool])

    events = [event async for event in runner.stream(**_stream_kwargs(conversation, workspace_id))]

    assert events[-1]["type"] == "final"
