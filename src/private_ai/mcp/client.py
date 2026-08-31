"""``McpHub`` — every tool the application can reach, from one object.

The eight built-in servers are mounted **in process**. Nothing is spawned and nothing is
dialled: ``create_server(services)`` hands each one the same live ``AppServices`` the UI
is using, so the agent and a standalone server talk to the same database, the same GPU
lease manager and the same graph store. Speaking MCP to ourselves over a socket would
buy nothing and would break the packaged desktop app, where no second process runs.

External servers the user configured are real MCP clients over stdio or streamable HTTP.
They are optional by construction: one that will not start is logged and skipped, because
a third-party server being down must not cost the user their own tools.
"""

from __future__ import annotations

import asyncio
import json
import logging
from contextlib import AsyncExitStack, suppress
from typing import TYPE_CHECKING, Any

from private_ai.mcp.adapter import alias_for, mcp_tools_to_langchain, render_result

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from langchain_core.tools import BaseTool
    from mcp.server.mcpserver import MCPServer

    from private_ai.core.services import AppServices

logger = logging.getLogger(__name__)

__all__ = ["AGENT_TOOLS", "ARTIFACT_TOOLS", "BUILTIN_SERVERS", "READ_ONLY_TOOLS", "McpHub"]

# module path -> server id. The id is what `servers()` reports and what a log line names.
BUILTIN_SERVERS: dict[str, str] = {
    "private_ai.mcp.servers.core_server": "core",
    "private_ai.mcp.servers.artifacts": "artifacts",
    "private_ai.mcp.servers.rag_vector": "rag.vector",
    "private_ai.mcp.servers.rag_keyword": "rag.keyword",
    "private_ai.mcp.servers.rag_hybrid": "rag.hybrid",
    "private_ai.mcp.servers.rag_graph": "rag.graph",
    "private_ai.mcp.servers.rag_summary": "rag.summary",
    "private_ai.mcp.servers.rag_web": "rag.web",
}

# Chat may look, never touch. Ingestion, deletion, memory writes and model defaults stay
# in the UI where the user performs them deliberately, so no document and no web page can
# talk a model into one of them.
READ_ONLY_TOOLS = frozenset(
    {
        "workspaces.list",
        "documents.list",
        "documents.status",
        "documents.get",
        "memory.list",
        "memory.search",
        "models.list",
        "models.status",
        "models.capabilities",
        "system.info",
        "system.time",
        "files.allowed",
        "files.list",
        "files.read",
        "rag.auto.search",
        "rag.vector.search",
        "rag.keyword.search",
        "rag.hybrid.search",
        "rag.graph.search",
        "rag.graph.neighborhood",
        "rag.graph.entities",
        "rag.summary.digest",
        "rag.summary.outline",
        "rag.web.search",
    }
)

# The one exception to "chat may look, never touch", and it is a narrow one. These write
# a new file into `data_dir/artifacts` and return its path; there is no tool here that
# opens, overwrites, moves or deletes anything, and nothing outside that folder is
# reachable. So the worst a document can talk a model into is an unwanted file in a
# folder that exists for exactly that — not a lost document and not a rewritten memory.
ARTIFACT_TOOLS = frozenset(
    {
        "artifacts.list",
        "artifacts.create_chart",
        "artifacts.create_diagram",
        "artifacts.create_document",
        "artifacts.create_slides",
    }
)

# What the agent is handed. `READ_ONLY_TOOLS` keeps its literal meaning so the invariant
# it exists for — no mutating tool is ever in it — stays checkable on its own.
AGENT_TOOLS = READ_ONLY_TOOLS | ARTIFACT_TOOLS

# Names an external server publishes are namespaced before the agent ever sees them, so a
# third-party tool can neither collide with a built-in name nor impersonate one that the
# read-only filter would have refused.
EXTERNAL_PREFIX = "ext"

# Lists every workspace, so it is the one tool a workspace-pinned agent must not have:
# it is how a turn learns that other workspaces exist. The UI still calls it directly.
WORKSPACE_DIRECTORY_TOOL = "workspaces.list"

CONNECT_TIMEOUT_SECONDS = 20.0


class _ExternalServer:
    """One configured MCP server, kept open by a task of its own.

    The transport context managers are anyio-scoped: entering one in ``start`` and exiting
    it from ``close`` — a different task — tears down a cancel scope in the wrong place.
    So the connection lives inside a single task that opens it, signals ready, and then
    waits to be told to stop.
    """

    def __init__(self, name: str) -> None:
        self.name = name
        self.prefix = f"{EXTERNAL_PREFIX}.{name}"
        self.session: Any = None
        self._ready = asyncio.Event()
        self._stop = asyncio.Event()
        self._task: asyncio.Task[None] | None = None
        self._error: BaseException | None = None

    async def connect(self, opener) -> None:
        self._task = asyncio.create_task(self._run(opener), name=f"mcp-{self.name}")
        ready = asyncio.create_task(self._ready.wait())
        done, _ = await asyncio.wait(
            {ready, self._task},
            timeout=CONNECT_TIMEOUT_SECONDS,
            return_when=asyncio.FIRST_COMPLETED,
        )
        ready.cancel()
        with suppress(asyncio.CancelledError):
            await ready
        if self._ready.is_set():
            return
        await self.close()
        if self._error is not None:
            raise self._error
        raise TimeoutError(f"MCP server {self.name} did not answer initialize in time")

    async def _run(self, opener) -> None:
        from mcp import ClientSession

        try:
            async with AsyncExitStack() as stack:
                streams = await stack.enter_async_context(opener())
                read_stream, write_stream = streams[0], streams[1]
                session = await stack.enter_async_context(ClientSession(read_stream, write_stream))
                await session.initialize()
                self.session = session
                self._ready.set()
                await self._stop.wait()
        except Exception as exc:  # noqa: BLE001 - reported to `connect`, never raised here
            self._error = exc
        finally:
            self.session = None
            self._ready.set()

    async def list_tools(self) -> list[Any]:
        """Namespaced copies of what the remote publishes, so names cannot collide."""
        if self.session is None:
            return []
        result = await self.session.list_tools()
        tools = []
        for tool in result.tools:
            renamed = tool.model_copy(update={"name": f"{self.prefix}.{tool.name}"})
            tools.append(renamed)
        return tools

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> Any:
        if self.session is None:
            raise RuntimeError(f"MCP server {self.name} is not connected")
        remote = name.removeprefix(f"{self.prefix}.")
        return await self.session.call_tool(remote, arguments)

    async def close(self) -> None:
        self._stop.set()
        task = self._task
        self._task = None
        if task is None:
            return
        task.cancel()
        with suppress(asyncio.CancelledError, Exception):
            await task


class McpHub:
    """Mounts the built-in servers, connects the configured ones, hands out tools."""

    def __init__(self, services: AppServices) -> None:
        self.services = services
        self._servers: dict[str, Any] = {}
        self._external: list[_ExternalServer] = []
        self._index: dict[str, Any] = {}
        self._started = False

    # --- lifecycle --------------------------------------------------------

    async def start(self) -> None:
        if self._started:
            return
        self._started = True
        for module_path, server_id in BUILTIN_SERVERS.items():
            try:
                self._servers[server_id] = _mount(module_path, self.services)
            except Exception:  # noqa: BLE001 - one broken server must not sink the rest
                logger.exception("Không dựng được MCP server nội bộ %s", server_id)
        for name, opener in _external_openers(self.services):
            external = _ExternalServer(name)
            try:
                await external.connect(opener)
            except Exception as exc:  # noqa: BLE001 - third-party servers are best effort
                logger.warning("Bỏ qua MCP server ngoài %s: %s", name, exc)
                continue
            self._external.append(external)
            self._servers[external.prefix] = external

    async def close(self) -> None:
        for external in self._external:
            await external.close()
        self._external.clear()
        self._servers.clear()
        self._index.clear()
        self._started = False

    def servers(self) -> list[str]:
        return list(self._servers)

    # --- tools ------------------------------------------------------------

    async def tools(
        self,
        *,
        allow: frozenset[str] | None = AGENT_TOOLS,
        workspace_id: str = "",
    ) -> list[BaseTool]:
        """Every advertised tool, as LangChain tools, minus anything ``allow`` excludes.

        ``allow`` defaults to :data:`AGENT_TOOLS` — everything read-only, plus the
        artifact writers — and is passed down to the adapter, which checks it again at
        invoke time. Filtering only here would leave a model free to call a mutating tool
        by guessing its mangled name.

        ``workspace_id`` confines the turn to one workspace: the id is stripped from every
        tool schema and forced at call time. ``workspaces.list`` is withheld entirely,
        because a pinned agent has no id to choose and enumerating the other workspaces is
        the discovery step that made cross-workspace reads possible in the first place.
        Documents belong to exactly one workspace, and a chat turn sees exactly one.

        An external server's tools are namespaced ``ext.<server>.<tool>`` and are not in
        the built-in allow set, so they are matched against the same set only when the
        caller supplied one that names them — the user configured that server on purpose,
        and its tools stand or fall on that decision rather than on our list.
        """
        await self.start()
        collected: list[BaseTool] = []
        for server_id, server in self._servers.items():
            external = server_id.startswith(f"{EXTERNAL_PREFIX}.")
            scope = None if external else allow
            if workspace_id and not external and scope is not None:
                scope = scope - {WORKSPACE_DIRECTORY_TOOL}
            try:
                collected.extend(
                    await mcp_tools_to_langchain(
                        server,
                        allow=scope,
                        workspace_id="" if external else workspace_id,
                    )
                )
            except Exception:  # noqa: BLE001 - a server that cannot list is a server we skip
                logger.exception("Không liệt kê được công cụ của MCP server %s", server_id)
        return collected

    async def call(self, name: str, arguments: dict[str, Any]) -> str:
        """Run one tool by its dotted (or mangled) name and return text.

        Used by the UI and by anything that already knows what it wants; the agent goes
        through the LangChain tools instead. Errors come back as text for the same reason
        they do there: the caller needs to read why.
        """
        await self.start()
        dotted = name.replace("__", ".")
        owner = await self._owner(dotted)
        if owner is None:
            return f"Tool {dotted} is not available."
        try:
            result = await owner.call_tool(dotted, arguments)
        except Exception as exc:  # noqa: BLE001 - a failing tool must not kill the caller
            return f"Tool {dotted} failed: {exc}"
        return render_result(result)

    async def _owner(self, dotted: str) -> Any:
        if dotted not in self._index:
            await self._reindex()
        return self._index.get(dotted)

    async def _reindex(self) -> None:
        """Which server owns which tool. Rebuilt only on a miss — the built-ins are fixed,
        but an external server can be reconnected under the same hub."""
        self._index.clear()
        for server in self._servers.values():
            with suppress(Exception):
                for tool in await server.list_tools():
                    self._index.setdefault(str(tool.name), server)


def _mount(module_path: str, services: AppServices) -> MCPServer:
    from importlib import import_module

    module = import_module(module_path)
    return module.create_server(services)


def _external_openers(services: AppServices):
    """Configured external servers, as ``(name, () -> transport context manager)``.

    Two sources, both optional: the ``mcp_servers`` table the UI writes, and the
    ``PRIVATE_AI_MCP_EXTERNAL_SERVERS`` JSON blob for a headless install with no UI.
    """
    entries: list[tuple[str, dict[str, Any]]] = []
    entries.extend(_settings_entries(services.settings.mcp_external_servers))
    entries.extend(_database_entries(services.database))

    seen: set[str] = set()
    openers = []
    for name, config in entries:
        if name in seen:
            continue
        seen.add(name)
        opener = _opener(config)
        if opener is None:
            logger.warning("MCP server ngoài %s thiếu command hoặc url", name)
            continue
        openers.append((name, opener))
    return openers


def _settings_entries(raw: str) -> list[tuple[str, dict[str, Any]]]:
    if not raw.strip():
        return []
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError as exc:
        logger.warning("PRIVATE_AI_MCP_EXTERNAL_SERVERS không phải JSON hợp lệ: %s", exc)
        return []
    if not isinstance(parsed, dict):
        return []
    return [(str(name), config) for name, config in parsed.items() if isinstance(config, dict)]


def _database_entries(database: Any) -> list[tuple[str, dict[str, Any]]]:
    try:
        rows = database.fetch_all(
            "SELECT name, kind, command, args_json, url, headers_json FROM mcp_servers "
            "WHERE enabled = 1 ORDER BY name"
        )
    except Exception as exc:  # noqa: BLE001 - a missing table must not stop the agent
        logger.warning("Không đọc được bảng mcp_servers: %s", exc)
        return []
    entries: list[tuple[str, dict[str, Any]]] = []
    for row in rows:
        if str(row["kind"]) == "builtin":
            continue
        entries.append(
            (
                str(row["name"]),
                {
                    "command": str(row["command"] or ""),
                    "args": _json_value(row["args_json"], list),
                    "url": str(row["url"] or ""),
                    "headers": _json_value(row["headers_json"], dict),
                },
            )
        )
    return entries


def _json_value(raw: Any, kind: type) -> Any:
    try:
        parsed = json.loads(str(raw or ("[]" if kind is list else "{}")))
    except json.JSONDecodeError:
        return kind()
    return parsed if isinstance(parsed, kind) else kind()


def _opener(config: dict[str, Any]):
    command = str(config.get("command") or "").strip()
    url = str(config.get("url") or "").strip()
    if command:
        from mcp.client.stdio import StdioServerParameters, stdio_client

        parameters = StdioServerParameters(
            command=command,
            args=[str(item) for item in config.get("args") or []],
            env={str(k): str(v) for k, v in (config.get("env") or {}).items()} or None,
            cwd=config.get("cwd") or None,
        )
        return lambda: stdio_client(parameters)
    if url:
        from mcp.client.streamable_http import create_mcp_http_client, streamable_http_client

        headers = {str(k): str(v) for k, v in (config.get("headers") or {}).items()}
        return lambda: streamable_http_client(
            url,
            http_client=create_mcp_http_client(headers=headers) if headers else None,
        )
    return None


def agent_tool_names() -> list[str]:
    """The mangled names the agent will see, for a prompt that lists them."""
    return sorted(alias_for(name) for name in AGENT_TOOLS)
