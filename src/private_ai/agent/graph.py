"""The chat turn as a LangGraph.

This replaces the hand-rolled four-round tool loop the HTTP router used to run. The
shape is the same and the guarantees are the same — retrieval happens once before the
model sees anything, the model may ask for tools a bounded number of times, and the last
round is offered no tools so it has to answer — but the loop, the transcript and the
cancellation semantics are LangGraph's rather than ours.

    plan → retrieve → agent ⇄ tools
                        ↓
                       END

``plan`` and ``retrieve`` make no model call. Choosing a strategy and guessing which
skills apply is keyword work (see ``StrategyRegistry`` auto and ``SkillRegistry.select``),
and paying for a router model call on every turn would double the latency of the cheap
questions to slightly improve the expensive ones.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from langchain_core.messages import AIMessage, AnyMessage, SystemMessage
from langgraph.graph import END, START, StateGraph
from langgraph.prebuilt import ToolNode, tools_condition

from private_ai.agent.prompts import build_system_prompt
from private_ai.agent.state import AgentState
from private_ai.core.schemas import Citation
from private_ai.rag.web_search import WebSearchResponse, WebSearchUnavailable

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from langchain_core.tools import BaseTool
    from langgraph.graph.state import CompiledStateGraph

    from private_ai.config import Settings
    from private_ai.core.services import AppServices

__all__ = ["agent_config", "build_agent_graph", "tool_rounds"]

SNIPPET_CHARS = 280
MEMORY_LIMIT = 5
# A digest covers a whole document, so it could cite hundreds of chunks. The answer only
# needs enough provenance to be checkable.
CITATION_LIMIT = 8


async def _summary_plan(services: AppServices, query: str, workspace_id: str, strategy: str):
    """The summary plan for this turn, or ``None`` if this is not an exhaustive summary.

    ``auto`` resolves the strategy itself and already pays for the scope lookup, so ask it
    rather than repeating the work. A scope error means a document was found but the
    requested part of it does not exist — that falls back to ordinary retrieval instead of
    failing the turn.
    """
    if strategy not in {"auto", "summary"}:
        return None
    try:
        if strategy == "summary":
            return await services.strategies.get("summary").scope(query, workspace_id)
        _chosen, _reason, plan = await services.strategies.get("auto").choose(
            query, workspace_id=workspace_id
        )
        return plan
    except Exception:
        return None


def tool_rounds(settings: Settings) -> int:
    """How many times the model may ask for tools before it is made to answer.

    A tool call plus the model turn that reads its result costs two steps of the
    recursion budget, and one step is reserved for the answer itself.
    """
    return max(1, (settings.agent_max_iterations - 1) // 2)


def agent_config(settings: Settings) -> dict[str, Any]:
    """The run config the graph must be invoked with.

    A full-budget turn spends ``plan`` + ``retrieve`` + one ``agent`` step per tool round +
    one ``tools`` step per round + the final answer, so it needs
    ``2 + 2 * tool_rounds + 1`` supersteps. At an odd ``agent_max_iterations`` that comes to
    exactly ``agent_max_iterations + 2``, and LangGraph raises once the count *reaches* the
    limit — so budgeting ``+2`` killed the last tool round on every odd setting.
    """
    return {"recursion_limit": settings.agent_max_iterations + 3}


async def build_agent_graph(
    services: AppServices,
    *,
    tools: list[BaseTool] | None = None,
) -> CompiledStateGraph:
    """Compile the turn graph. ``tools`` defaults to the agent-visible MCP tool set."""
    settings = services.settings
    if tools is None:
        tools = await services.mcp.tools() if services.mcp is not None else []
    rounds = tool_rounds(settings)

    def plan(state: AgentState) -> dict[str, Any]:
        strategy = state.get("strategy") or settings.retrieval_default_strategy
        if strategy not in set(services.strategies.names()):
            strategy = "auto"
        requested = state.get("skills") or []
        if requested:
            chosen = [services.skills.get(name) for name in requested]
            selected = [skill for skill in chosen if skill is not None]
        else:
            selected = services.skills.select(state.get("query", ""))
        return {"strategy": strategy, "skills": [skill.name for skill in selected]}

    async def retrieve(state: AgentState) -> dict[str, Any]:
        query = state.get("query", "")
        workspace_id = state.get("workspace_id", "")
        notices: list[str] = []

        documents = []
        summary = ""
        summary_label = ""
        try:
            plan = await _summary_plan(services, query, workspace_id, state.get("strategy", "auto"))
            if plan is not None:
                # An exhaustive summary is reduced *before* the prompt is built. Handing
                # the model every chunk instead is what used to overflow the context
                # window: the strategy returns the whole document by design, and only a
                # map-reduce brings it back under budget.
                summary_label = plan.source_label
                summary = await services.strategies.get("summary").digest(
                    query,
                    workspace_id,
                    model=state.get("model", ""),
                    plan=plan,
                )
                documents = services.strategies.get("summary").documents(plan)[:CITATION_LIMIT]
                notices.append(
                    f"Đã đọc và tóm tắt {len(plan.chunks)} đoạn của [{plan.source_label}]."
                )
            else:
                documents = await services.strategies.retrieve(
                    query,
                    workspace_id=workspace_id,
                    strategy=state.get("strategy", "auto"),
                    limit=settings.retrieval_top_k,
                )
        except Exception as exc:  # a dead index must not take the answer with it
            notices.append(f"Không truy hồi được tài liệu: {exc}")

        memories = []
        try:
            memories = await services.memory.search(
                query,
                user_id=state.get("user_id", ""),
                limit=MEMORY_LIMIT,
            )
        except Exception as exc:
            notices.append(f"Không đọc được bộ nhớ cá nhân: {exc}")

        web = await _web_context(services, state, notices) if state.get("web_search") else None

        activated = [services.skills.get(name) for name in state.get("skills", [])]
        prompt = build_system_prompt(
            documents=documents,
            memories=memories,
            web=web,
            skills=services.skills,
            activated=[skill for skill in activated if skill is not None],
            summary=summary,
            summary_label=summary_label,
            budget=settings.retrieval_context_chars,
        )
        citations = [
            Citation.from_metadata(
                document.metadata,
                document.page_content[:SNIPPET_CHARS],
            ).model_dump(mode="json")
            for document in documents
        ]
        return {
            "documents": documents,
            "citations": citations,
            "system_prompt": prompt,
            "notices": notices,
        }

    async def agent(state: AgentState) -> dict[str, Any]:
        iterations = int(state.get("iterations", 0))
        history: list[AnyMessage] = list(state.get("messages", []))
        prompt = state.get("system_prompt", "")
        messages: list[AnyMessage] = [SystemMessage(prompt), *history] if prompt else history
        # The final round drops the tools, so the model has to answer instead of asking
        # for one more thing it will not get.
        offered = tools if tools and iterations < rounds else None
        model = services.models.chat_model(
            state.get("model", ""),
            streaming=settings.agent_stream_tokens,
            tools=offered,
        )
        response = await _call_model(model, messages, stream=settings.agent_stream_tokens)
        return {"messages": [response], "iterations": iterations + 1}

    def route(state: AgentState) -> str:
        # The tool-free round has been taken, so whatever came back is the answer. A model
        # that asks for a tool it was not offered would otherwise loop until the recursion
        # limit turned a finished turn into an error.
        if int(state.get("iterations", 0)) > rounds:
            return END
        return tools_condition(state)

    graph = StateGraph(AgentState)
    graph.add_node("plan", plan)
    graph.add_node("retrieve", retrieve)
    graph.add_node("agent", agent)
    graph.add_edge(START, "plan")
    graph.add_edge("plan", "retrieve")
    graph.add_edge("retrieve", "agent")
    if tools:
        graph.add_node("tools", ToolNode(tools))
        graph.add_conditional_edges("agent", route, {"tools": "tools", END: END})
        graph.add_edge("tools", "agent")
    else:
        graph.add_edge("agent", END)
    return graph.compile()


async def _web_context(
    services: AppServices,
    state: AgentState,
    notices: list[str],
) -> WebSearchResponse | None:
    """Search the web only when this message asked for it, and never fail the turn over it."""
    try:
        found = await services.web_search.search(state.get("query", ""))
    except WebSearchUnavailable as exc:
        notices.append(str(exc))
        return None
    except Exception as exc:
        notices.append(f"Tìm kiếm web thất bại: {exc}")
        return None
    if not found.results and not found.summary:
        notices.append("Tìm kiếm web không trả về kết quả nào.")
        return None
    return found


async def _call_model(model: Any, messages: list[AnyMessage], *, stream: bool) -> AnyMessage:
    """One model turn, streamed when the provider can do it.

    ``astream`` is what puts tokens on LangGraph's ``messages`` channel; a provider that
    cannot stream falls back to a single chunk, which is why the chunks are folded rather
    than assumed to be addable.
    """
    if not stream:
        return await model.ainvoke(messages)
    collected: AnyMessage | None = None
    async for chunk in model.astream(messages):
        collected = chunk if collected is None else collected + chunk
    return collected if collected is not None else AIMessage(content="")
