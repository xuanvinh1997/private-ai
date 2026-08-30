"""What one chat turn carries between the graph's nodes.

Only ``messages`` accumulates the way LangGraph's own agents do. Everything else is
either decided once by ``plan`` and read afterwards, or appended by ``retrieve`` — hence
the two additive reducers: several nodes may contribute a notice or a citation and none
of them should clobber the others.
"""

from __future__ import annotations

import operator
from typing import Annotated, Any, TypedDict

from langchain_core.documents import Document
from langchain_core.messages import AnyMessage
from langgraph.graph.message import add_messages


class AgentState(TypedDict, total=False):
    """The graph state. ``total=False`` because ``plan`` fills most of it in."""

    messages: Annotated[list[AnyMessage], add_messages]

    # Set by the caller, read everywhere.
    query: str
    workspace_id: str
    conversation_id: str
    user_id: str
    model: str
    web_search: bool

    # Decided by ``plan``.
    strategy: str
    skills: list[str]

    # Produced by ``retrieve``.
    documents: Annotated[list[Document], operator.add]
    citations: Annotated[list[dict[str, Any]], operator.add]
    system_prompt: str

    # Anything the user should be told that is not part of the answer — a failed web
    # search, a strategy that came back empty. Never fatal.
    notices: Annotated[list[str], operator.add]

    # Model turns taken so far. The last one is offered no tools, which is what forces
    # an answer instead of another tool request.
    iterations: int
    tools_used: Annotated[list[str], operator.add]


def initial_state(
    *,
    query: str,
    workspace_id: str,
    conversation_id: str,
    user_id: str,
    history: list[AnyMessage],
    model: str = "",
    strategy: str = "auto",
    web_search: bool = False,
    skills: list[str] | None = None,
) -> AgentState:
    return AgentState(
        messages=history,
        query=query,
        workspace_id=workspace_id,
        conversation_id=conversation_id,
        user_id=user_id,
        model=model,
        web_search=web_search,
        strategy=strategy,
        skills=list(skills or []),
        documents=[],
        citations=[],
        system_prompt="",
        notices=[],
        iterations=0,
        tools_used=[],
    )
