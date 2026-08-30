"""MCP: the tool surface Private AI exposes, and the bridge that hands it to the agent.

Seven servers live under ``servers/`` — one for the non-retrieval tools and one per
retrieval strategy. Splitting them is what lets a model choose a retriever by reading
descriptions instead of being handed a single opaque ``search`` tool.
"""

from __future__ import annotations

from private_ai.mcp.adapter import (
    MAX_TOOL_OUTPUT_CHARS,
    NAME_SEPARATOR,
    alias_for,
    mcp_tools_to_langchain,
    name_for,
)
from private_ai.mcp.client import BUILTIN_SERVERS, READ_ONLY_TOOLS, McpHub
from private_ai.mcp.common import build_server, serve_http, serve_stdio

__all__ = [
    "BUILTIN_SERVERS",
    "MAX_TOOL_OUTPUT_CHARS",
    "NAME_SEPARATOR",
    "READ_ONLY_TOOLS",
    "McpHub",
    "alias_for",
    "build_server",
    "mcp_tools_to_langchain",
    "name_for",
    "serve_http",
    "serve_stdio",
]
