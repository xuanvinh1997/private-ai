from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any

from private_ai_api.schemas import ChatMessage, ChatRequest, ToolCall

# Function names may only contain letters, digits, underscores and dashes on the OpenAI
# wire format, but every tool here is named with a dot. The alias is what the model sees.
NAME_SEPARATOR = "__"
MAX_TOOL_ROUNDS = 4
MAX_TOOL_OUTPUT_CHARS = 6000

# Chat may look, never touch. Ingestion, deletion and memory writes stay in the UI where the
# user performs them deliberately, so a model cannot be talked into them by a document.
READ_ONLY_TOOLS = frozenset(
    {
        "system.info",
        "system.time",
        "files.allowed",
        "files.list",
        "files.read",
        "web.search",
        "workspaces.list",
        "documents.list",
        "documents.status",
        "documents.search",
        "graph.search",
        "graph.find_entity",
        "graph.neighborhood",
        "graph.answer",
        "memory.list",
        "memory.search",
        "models.list",
        "models.status",
        "models.capabilities",
    }
)


def alias_for(name: str) -> str:
    return name.replace(".", NAME_SEPARATOR)


class McpToolBridge:
    """Exposes the local MCP tools to a chat model and runs the calls it asks for.

    The MCP server is used in-process: the chat path is not an MCP client over the network,
    so it works in the packaged desktop app where no separate MCP process is running.
    """

    def __init__(self, server: Any, allowed: frozenset[str] = READ_ONLY_TOOLS) -> None:
        self.server = server
        self.allowed = allowed
        self._specs: list[dict[str, Any]] | None = None
        self._names: dict[str, str] = {}

    async def specs(self) -> list[dict[str, Any]]:
        """The tool list in the shape both providers accept, built once per process."""
        if self._specs is not None:
            return self._specs
        specs: list[dict[str, Any]] = []
        names: dict[str, str] = {}
        for tool in await self.server.list_tools():
            if tool.name not in self.allowed:
                continue
            alias = alias_for(tool.name)
            names[alias] = tool.name
            specs.append(
                {
                    "type": "function",
                    "function": {
                        "name": alias,
                        "description": (tool.description or "").strip(),
                        "parameters": tool.input_schema,
                    },
                }
            )
        self._specs = specs
        self._names = names
        return specs

    async def invoke(self, alias: str, arguments: dict[str, Any]) -> str:
        """Run one tool and return text the model can read, errors included.

        A refused call is an answer, not a crash: the model needs to read why so it can pick
        different arguments or tell the user what is missing.
        """
        from mcp.server.mcpserver.exceptions import ToolError

        await self.specs()
        name = self._names.get(alias, alias)
        if name not in self.allowed:
            return f"Tool {alias} is not available to chat."
        try:
            result = await self.server.call_tool(name, arguments)
        except ToolError as exc:
            return f"Tool {name} failed: {exc}"
        except Exception as exc:  # noqa: BLE001 - the model gets the reason, the chat survives
            return f"Tool {name} failed unexpectedly: {exc}"
        payload = getattr(result, "structured_content", None)
        if payload is None:
            payload = getattr(result, "content", result)
        try:
            text = json.dumps(payload, ensure_ascii=False, default=str)
        except (TypeError, ValueError):
            text = str(payload)
        return text[:MAX_TOOL_OUTPUT_CHARS]


def read_tool_calls(message: dict[str, Any]) -> list[ToolCall]:
    """Normalize the two dialects' tool calls into one shape."""
    calls: list[ToolCall] = []
    for index, raw in enumerate(message.get("tool_calls") or []):
        function = raw.get("function") or {}
        arguments = function.get("arguments")
        if isinstance(arguments, str):
            try:
                arguments = json.loads(arguments or "{}")
            except json.JSONDecodeError:
                arguments = {}
        name = str(function.get("name") or "")
        if not name:
            continue
        calls.append(
            ToolCall(
                id=str(raw.get("id") or f"call_{index}"),
                name=name,
                arguments=arguments if isinstance(arguments, dict) else {},
            )
        )
    return calls


async def run_tool_calls(
    bridge: McpToolBridge,
    calls: list[ToolCall],
    *,
    on_event: Callable[[dict[str, Any]], None] | None = None,
) -> list[ChatMessage]:
    """Execute one round of calls and return the assistant/tool messages they produce."""
    messages = [ChatMessage(role="assistant", content="", tool_calls=calls)]
    for call in calls:
        output = await bridge.invoke(call.name, call.arguments)
        if on_event:
            on_event({"tool": call.name.replace(NAME_SEPARATOR, "."), "arguments": call.arguments})
        messages.append(
            ChatMessage(
                role="tool",
                content=output,
                tool_call_id=call.id,
                name=call.name,
            )
        )
    return messages


def with_tools(request: ChatRequest, messages: list[ChatMessage], specs: list[dict[str, Any]]):
    """A copy of the request carrying the running transcript and the tool list."""
    return ChatRequest(
        model=request.model,
        messages=messages,
        stream=request.stream,
        options=request.options,
        tools=specs or None,
    )
