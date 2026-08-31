"""The one entry point the UI has into the AI.

``stream`` is what the chat view consumes: it turns LangGraph's two output channels into
the flat event dicts the contract fixes, and it owns everything around the run that the
graph itself has no business knowing about — writing the user's message down before the
model sees it, saving the answer afterwards, naming a fresh conversation, and leaving a
row in ``agent_runs`` describing what happened.

Partial answers survive cancellation. When the user closes the view or hits stop
mid-stream the generator is thrown a ``CancelledError``; whatever the model had already
said is written down in a synchronous ``finally`` — synchronous because awaiting during
cancellation is how you lose the very thing you were trying to save — and the error is
re-raised so the caller still sees a cancelled task.
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncIterator
from datetime import UTC, datetime
from typing import TYPE_CHECKING, Any
from uuid import uuid4

from langchain_core.messages import (
    AIMessage,
    AIMessageChunk,
    AnyMessage,
    HumanMessage,
    SystemMessage,
    ToolMessage,
)
from langgraph.errors import GraphRecursionError

from private_ai.agent import progress
from private_ai.agent.graph import agent_config, build_agent_graph
from private_ai.agent.state import initial_state
from private_ai.core import repositories
from private_ai.core.repositories import DEFAULT_CONVERSATION_TITLE
from private_ai.llm import InsufficientVram, NoProviderConfigured, ProviderUnavailable

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

    from langgraph.graph.state import CompiledStateGraph

    from private_ai.core.services import AppServices

__all__ = ["AgentRunner"]

EMPTY_ANSWER = "Mô hình không trả về nội dung nào"
NO_PROVIDER = "Chưa cấu hình nhà cung cấp AI nào"
PROVIDER_DOWN = "Không kết nối được nhà cung cấp AI đang chọn"
NO_VRAM = "Không đủ dung lượng GPU đã đặt trước"
TOO_MANY_STEPS = "Trợ lý đã dùng hết số vòng gọi công cụ cho phép mà chưa trả lời xong"

MAX_TOOL_OUTPUT_CHARS = 6000

_ROLE_MESSAGES = {
    "user": HumanMessage,
    "assistant": AIMessage,
    "system": SystemMessage,
}


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _text(content: Any) -> str:
    """Flatten a message's content, which may be a string or a list of typed blocks."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for block in content:
            if isinstance(block, str):
                parts.append(block)
            elif isinstance(block, dict) and block.get("type") == "text":
                parts.append(str(block.get("text", "")))
        return "".join(parts)
    return ""


def _history(messages: Sequence[Any]) -> list[AnyMessage]:
    converted: list[AnyMessage] = []
    for message in messages:
        factory = _ROLE_MESSAGES.get(str(message.role))
        if factory is not None:
            converted.append(factory(content=str(message.content)))
    return converted


def _progress_event(payload: Any) -> dict[str, Any] | None:
    """One custom-channel payload, as the event the UI consumes.

    Anything on that channel that is not ours is dropped rather than forwarded: a tool or
    a sub-graph may write to it too, and an unrecognised shape reaching the chat bubble
    would render as an empty step.
    """
    if not isinstance(payload, dict):
        return None
    kind = str(payload.get("kind") or "")
    if kind == "notice":
        message = str(payload.get("message") or "").strip()
        return {"type": "notice", "message": message} if message else None
    if kind != "progress":
        return None
    event: dict[str, Any] = {
        "type": "progress",
        "stage": str(payload.get("stage") or ""),
        "label": str(payload.get("label") or ""),
        "detail": str(payload.get("detail") or ""),
    }
    fraction = payload.get("fraction")
    if isinstance(fraction, int | float):
        event["fraction"] = float(fraction)
    return event


class AgentRunner:
    """Runs one chat turn and reports it as a stream of events."""

    def __init__(self, services: AppServices) -> None:
        self.services = services
        # One graph per workspace: the workspace is bound into the tool schemas, so a
        # single shared graph would hand every turn the first workspace's tools.
        self._graphs: dict[str, CompiledStateGraph] = {}
        self._build_lock = asyncio.Lock()

    async def graph(self, workspace_id: str = "") -> CompiledStateGraph:
        """Compiled once per workspace. Mounting an MCP server calls :meth:`reset`."""
        cached = self._graphs.get(workspace_id)
        if cached is None:
            async with self._build_lock:
                cached = self._graphs.get(workspace_id)
                if cached is None:
                    cached = await build_agent_graph(self.services, workspace_id=workspace_id)
                    self._graphs[workspace_id] = cached
        return cached

    def reset(self) -> None:
        self._graphs.clear()

    # --- the public stream -------------------------------------------------

    async def stream(
        self,
        *,
        conversation_id: str,
        content: str,
        workspace_id: str,
        model: str = "",
        strategy: str = "auto",
        web_search: bool = False,
        skills: Sequence[str] | None = None,
    ) -> AsyncIterator[dict[str, Any]]:
        services = self.services
        database = services.database
        settings = services.settings

        conversation = await repositories.get_conversation(database, conversation_id)
        history = _history(conversation.messages)
        await repositories.append_message(database, conversation_id, "user", content)
        user_id = await repositories.active_profile_id_async(database)

        state = initial_state(
            query=content,
            workspace_id=workspace_id or conversation.workspace_id,
            conversation_id=conversation_id,
            user_id=user_id,
            history=[*history, HumanMessage(content=content)],
            model=model,
            strategy=strategy,
            web_search=web_search,
            skills=list(skills or []),
        )
        run_id = self._open_run(conversation_id, strategy)

        # The graph carries workspace-bound tools, so it must match the turn's workspace.
        graph = await self.graph(state["workspace_id"])
        answer_parts: list[str] = []
        citations: list[dict[str, Any]] = []
        used_skills: list[str] = []
        used_tools: list[str] = []
        seen_notices: set[str] = set()
        chosen_strategy = strategy
        routed_to = ""
        routing_reason = ""
        saved = False
        status = "completed"
        failure = ""

        try:
            async for mode, payload in graph.astream(
                state,
                config=agent_config(settings),
                stream_mode=["messages", "updates", "custom"],
            ):
                # The nodes write their own progress on the custom channel, so it arrives
                # interleaved with tokens in the order it happened rather than on a second
                # path the UI would have to re-order.
                if mode == "custom":
                    event = _progress_event(payload)
                    if event is not None:
                        yield event
                    continue
                if mode == "messages":
                    message, metadata = payload
                    if metadata.get("langgraph_node") != "agent":
                        continue
                    if not isinstance(message, AIMessage | AIMessageChunk):
                        continue
                    piece = _text(message.content)
                    if piece:
                        answer_parts.append(piece)
                        yield {"type": "token", "content": piece}
                    continue

                for node, update in payload.items():
                    if not isinstance(update, dict):
                        continue
                    if node == "plan":
                        chosen_strategy = str(update.get("strategy") or chosen_strategy)
                        used_skills = [str(name) for name in update.get("skills") or []]
                    elif node == "retrieve":
                        citations.extend(update.get("citations") or [])
                        routed_to = str(update.get("routed_to") or routed_to)
                        routing_reason = str(update.get("routing_reason") or routing_reason)
                        # Streamed live from the node as well; this is the catch-up for a
                        # notice recorded before anything was listening.
                        for notice in update.get("notices") or []:
                            text = str(notice)
                            if text not in seen_notices:
                                seen_notices.add(text)
                                yield {"type": "notice", "message": text}
                    elif node == "agent":
                        for message in update.get("messages") or []:
                            if not settings.agent_stream_tokens:
                                piece = _text(message.content)
                                if piece:
                                    answer_parts.append(piece)
                            # Announced before the tool runs: a slow tool would otherwise
                            # look like the answer had hung.
                            for call in getattr(message, "tool_calls", None) or []:
                                used_tools.append(str(call.get("name") or ""))
                                yield {
                                    "type": "progress",
                                    "stage": "tool",
                                    "label": progress.stage_label("tool"),
                                    "detail": str(call.get("name") or ""),
                                }
                                yield {
                                    "type": "tool_start",
                                    "name": str(call.get("name") or ""),
                                    "args": dict(call.get("args") or {}),
                                }
                    elif node == "tools":
                        for message in update.get("messages") or []:
                            if isinstance(message, ToolMessage):
                                yield {
                                    "type": "tool_end",
                                    "name": str(message.name or ""),
                                    "output": _text(message.content)[:MAX_TOOL_OUTPUT_CHARS],
                                }

            answer = "".join(answer_parts).strip()
            if not answer:
                status = "failed"
                failure = EMPTY_ANSWER
                yield {"type": "error", "message": EMPTY_ANSWER}
            else:
                await asyncio.to_thread(self._finalize, conversation_id, content, answer, model)
                saved = True
                yield {
                    "type": "final",
                    "content": answer,
                    "citations": citations,
                    # What the turn asked for, what it actually used, and why: with
                    # ``auto`` the first two differ, and only the third explains it.
                    "strategy": chosen_strategy,
                    "routed_to": routed_to,
                    "routing_reason": routing_reason,
                }
        except asyncio.CancelledError:
            status = "cancelled"
            failure = "cancelled"
            raise
        except (
            NoProviderConfigured,
            ProviderUnavailable,
            InsufficientVram,
            GraphRecursionError,
        ) as exc:
            status = "failed"
            failure = str(exc) or type(exc).__name__
            yield {"type": "error", "message": _message_for(exc)}
        except Exception as exc:  # the turn fails, the app does not
            status = "failed"
            failure = f"{type(exc).__name__}: {exc}"
            yield {"type": "error", "message": str(exc) or type(exc).__name__}
        finally:
            # Synchronous on purpose: this also runs while the task is being cancelled,
            # and an await here would be cancelled too, losing the partial answer.
            partial = "".join(answer_parts).strip()
            if partial and not saved:
                self._finalize(conversation_id, content, partial, model)
            self._close_run(run_id, chosen_strategy, used_skills, used_tools, status, failure)

    async def run(self, **kwargs: Any) -> dict[str, Any]:
        """Collect a whole turn. Same events, no streaming — used by tests and MCP callers."""
        content = ""
        citations: list[dict[str, Any]] = []
        notices: list[str] = []
        routed_to = ""
        routing_reason = ""
        error = ""
        async for event in self.stream(**kwargs):
            kind = event.get("type")
            if kind == "final":
                content = str(event.get("content", ""))
                citations = list(event.get("citations") or [])
                routed_to = str(event.get("routed_to") or "")
                routing_reason = str(event.get("routing_reason") or "")
            elif kind == "notice":
                notices.append(str(event.get("message", "")))
            elif kind == "error":
                error = str(event.get("message", ""))
        return {
            "content": content,
            "citations": citations,
            "notices": notices,
            "routed_to": routed_to,
            "routing_reason": routing_reason,
            "error": error,
        }

    # --- persistence -------------------------------------------------------

    def _finalize(self, conversation_id: str, question: str, answer: str, model: str) -> None:
        """Write the answer down, name the conversation, float it to the top of the list."""
        database = self.services.database
        now = _now()
        database.execute(
            "INSERT INTO messages(id, conversation_id, role, content, created_at) "
            "VALUES (?, ?, 'assistant', ?, ?)",
            (str(uuid4()), conversation_id, answer, now),
        )
        row = database.fetch_one(
            "SELECT title FROM conversations WHERE id = ?",
            (conversation_id,),
        )
        title = str(row["title"]) if row else DEFAULT_CONVERSATION_TITLE
        if title == DEFAULT_CONVERSATION_TITLE:
            title = question.strip().replace("\n", " ")[:80] or title
        if model:
            database.execute(
                "UPDATE conversations SET title = ?, model = ?, updated_at = ? WHERE id = ?",
                (title, model, now, conversation_id),
            )
        else:
            database.execute(
                "UPDATE conversations SET title = ?, updated_at = ? WHERE id = ?",
                (title, now, conversation_id),
            )
        database.execute(
            "UPDATE workspaces SET updated_at = ? "
            "WHERE id = (SELECT workspace_id FROM conversations WHERE id = ?)",
            (now, conversation_id),
        )

    def _open_run(self, conversation_id: str, strategy: str) -> str:
        run_id = str(uuid4())
        self.services.database.execute(
            "INSERT INTO agent_runs(id, conversation_id, strategy, status, started_at) "
            "VALUES (?, ?, ?, 'running', ?)",
            (run_id, conversation_id, strategy, _now()),
        )
        return run_id

    def _close_run(
        self,
        run_id: str,
        strategy: str,
        skills: list[str],
        tools: list[str],
        status: str,
        error: str,
    ) -> None:
        self.services.database.execute(
            """
            UPDATE agent_runs
            SET strategy = ?, skills_json = ?, tools_json = ?, status = ?, error = ?,
                finished_at = ?
            WHERE id = ?
            """,
            (
                strategy,
                json.dumps(skills, ensure_ascii=False),
                json.dumps(tools, ensure_ascii=False),
                status,
                error or None,
                _now(),
                run_id,
            ),
        )


def _message_for(exc: BaseException) -> str:
    if isinstance(exc, NoProviderConfigured):
        return NO_PROVIDER
    if isinstance(exc, ProviderUnavailable):
        return PROVIDER_DOWN
    if isinstance(exc, InsufficientVram):
        return NO_VRAM
    if isinstance(exc, GraphRecursionError):
        return TOO_MANY_STEPS
    return str(exc)
