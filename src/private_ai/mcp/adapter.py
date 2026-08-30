"""MCP tools, as LangChain tools.

``langchain-mcp-adapters`` would do this, but it pins ``mcp<2`` and installing it would
downgrade the server API this whole package is written against. This bridge is fifty
lines and we control what it refuses, which is the part that matters.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING, Any, Protocol

from langchain_core.tools import StructuredTool

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Sequence

# Function names on the OpenAI wire format may only hold letters, digits, underscores and
# dashes, and every tool here is named with dots. The alias is what the model sees; the
# dotted name is what MCP is asked for.
NAME_SEPARATOR = "__"
MAX_TOOL_OUTPUT_CHARS = 6000

__all__ = [
    "MAX_TOOL_OUTPUT_CHARS",
    "NAME_SEPARATOR",
    "alias_for",
    "invoker",
    "mcp_tools_to_langchain",
    "name_for",
    "render_result",
]


class ToolServer(Protocol):
    """The slice of ``MCPServer`` this bridge uses, so a remote session can stand in."""

    async def list_tools(self) -> Sequence[Any]: ...

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> Any: ...


def alias_for(name: str) -> str:
    return name.replace(".", NAME_SEPARATOR)


def name_for(alias: str) -> str:
    return alias.replace(NAME_SEPARATOR, ".")


def render_result(result: Any) -> str:
    """Whatever the tool returned, as text a model can read.

    ``structured_content`` first: it is the tool's own return value, already typed. The
    content blocks are the fallback for a tool that only produced text.
    """
    payload = getattr(result, "structured_content", None)
    if payload is None:
        payload = getattr(result, "content", result)
    try:
        text = json.dumps(payload, ensure_ascii=False, default=str)
    except (TypeError, ValueError):
        text = str(payload)
    return text[:MAX_TOOL_OUTPUT_CHARS]


def _object_schema(schema: Any) -> dict[str, Any]:
    """MCP guarantees an object schema; a server that sends something else gets a stub."""
    if isinstance(schema, dict) and schema.get("type") == "object":
        return schema
    return {"type": "object", "properties": {}}


async def mcp_tools_to_langchain(
    server: ToolServer,
    *,
    allow: frozenset[str] | None = None,
) -> list[StructuredTool]:
    """Advertise one server's tools to the agent, minus anything ``allow`` excludes."""
    tools: list[StructuredTool] = []
    for tool in await server.list_tools():
        name = str(tool.name)
        if allow is not None and name not in allow:
            continue
        tools.append(
            StructuredTool(
                name=alias_for(name),
                description=(tool.description or "").strip(),
                args_schema=_object_schema(getattr(tool, "input_schema", None)),
                coroutine=invoker(server, alias_for(name), allow=allow),
                func=None,
            )
        )
    return tools


def invoker(server: ToolServer, alias: str, *, allow: frozenset[str] | None = None):
    """The coroutine behind one advertised tool.

    The membership test runs again here, *after* unmangling, because the advertised list
    is only a hint: a model that guesses ``documents__delete`` reaches this function
    directly, and this is the layer that actually places the call.

    A refusal or a failure comes back as text rather than as an exception. The model has
    to read why it did not work — a raised error just ends the turn with nothing said.
    """
    name = name_for(alias)

    async def _run(**arguments: Any) -> str:
        if allow is not None and name not in allow:
            return f"Tool {name} is not available to the agent."
        try:
            result = await server.call_tool(name, arguments)
        except Exception as exc:  # noqa: BLE001 - a failing tool must not kill the turn
            return f"Tool {name} failed: {exc}"
        return render_result(result)

    return _run
