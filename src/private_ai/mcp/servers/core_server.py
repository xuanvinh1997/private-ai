"""The non-retrieval half of the tool surface: workspaces, documents, memory, models,
this machine, and the user's files.

Everything here reads off the live ``AppServices``. The rules the old single server
established are kept exactly: destructive tools refuse without ``confirmed=true``, the
file tools ask the user at the moment they first reach for a path, and the file holding
the MCP token is never readable through any of them.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING, Any

from mcp.server.elicitation import AcceptedElicitation
from mcp.server.mcpserver import Context, MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from pydantic import BaseModel, Field

from private_ai.core import repositories
from private_ai.core.file_access import FileAccessError
from private_ai.core.schemas import MemoryType
from private_ai.core.system_info import machine_snapshot, time_snapshot
from private_ai.mcp.common import (
    UNTRUSTED_FRAMING,
    build_server,
    require_workspace,
    resolve_services,
    results_payload,
    stdio_entry,
)

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from private_ai.core.services import AppServices

SERVER_NAME = "private-ai-core"

INSTRUCTIONS = (
    "Local tools for this machine's workspaces, documents, memory, models and files. "
    "All data stays on this machine. Destructive tools require confirmed=true. "
    "Model deletion and arbitrary database queries are intentionally unavailable.\n"
    "Retrieval lives on the rag.* servers; rag.auto.search here routes to whichever of "
    "them fits the question when you have no reason to pick one yourself."
)

# A document's extracted text is the whole file. `documents.get` exists so a model can
# read a short note in full, not so it can pull a 400-page PDF into the context window.
MAX_DOCUMENT_TEXT_CHARS = 20_000

# Columns worth showing for a document. `extracted_text` is deliberately not among them.
_DOCUMENT_FIELDS = (
    "id",
    "workspace_id",
    "filename",
    "media_type",
    "byte_size",
    "status",
    "error",
    "index_mode",
    "graph_model",
    "created_at",
    "updated_at",
    "indexed_at",
)


class FileAccessDecision(BaseModel):
    """What the user is asked when a tool reaches for a path they have not allowed yet."""

    allow: bool = Field(description="Cho phép Private AI đọc đường dẫn này")
    remember_folder: bool = Field(
        default=False,
        description="Nhớ thư mục này để lần sau không phải hỏi lại",
    )


def _document_view(row: dict[str, Any]) -> dict[str, Any]:
    return {key: row.get(key) for key in _DOCUMENT_FIELDS if key in row}


def create_server(services: AppServices | None = None) -> MCPServer:
    app = resolve_services(services)
    settings = app.settings
    database = app.database
    server = build_server(SERVER_NAME, "Private AI local tools", INSTRUCTIONS, settings=settings)

    # --- workspaces -------------------------------------------------------

    @server.tool(name="workspaces.list")
    async def list_workspaces() -> list[dict[str, Any]]:
        """List workspaces. Every document and retrieval tool needs one of these IDs."""
        return [
            workspace.model_dump(mode="json")
            for workspace in await repositories.list_workspaces(database)
        ]

    # --- documents --------------------------------------------------------

    @server.tool(name="documents.list")
    async def list_documents(
        workspace_id: str,
        q: str = "",
        status: str = "",
        limit: int = 20,
        offset: int = 0,
    ) -> dict[str, Any]:
        """List one workspace's documents without returning their extracted text.

        Use this to find out what is in the library and to get a document_id. To read a
        document's content, search it with a rag.* tool instead of fetching it whole.
        """
        await require_workspace(database, workspace_id)
        page = await repositories.list_documents(
            database,
            workspace_id,
            q=q,
            status=status,
            limit=limit,
            offset=offset,
        )
        return {
            "items": [_document_view(item) for item in page["items"]],
            "total": page["total"],
            "limit": page["limit"],
            "offset": page["offset"],
            "summary": page["summary"],
        }

    @server.tool(name="documents.status")
    async def document_status(document_id: str) -> dict[str, Any]:
        """Ingestion and indexing state for one document: is it ready to be searched?"""
        try:
            document = await repositories.get_document(database, document_id)
        except repositories.NotFound as exc:
            raise ToolError(str(exc)) from exc
        counts = await database.fetch_one_async(
            """
            SELECT COUNT(*) AS chunks,
                   COALESCE(SUM(embedding_vector IS NOT NULL), 0) AS embedded_chunks
            FROM document_chunks WHERE document_id = ?
            """,
            (document_id,),
        )
        return {
            **_document_view(document),
            **(counts or {"chunks": 0, "embedded_chunks": 0}),
            "ingestion": document.get("ingestion"),
        }

    @server.tool(
        name="documents.get",
        description=(
            "Read one document's extracted text, truncated.\n\n"
            "Only worth calling for a short document you already identified. For a "
            "question about a long one, retrieve passages with a rag.* tool: this "
            "returns the head of the file, not the part that answers the question.\n\n"
            f"{UNTRUSTED_FRAMING}"
        ),
    )
    async def get_document(document_id: str, max_chars: int = 0) -> dict[str, Any]:
        try:
            document = await repositories.get_document(database, document_id)
        except repositories.NotFound as exc:
            raise ToolError(str(exc)) from exc
        text = str(document.get("extracted_text") or "")
        cap = max(1, min(max_chars or MAX_DOCUMENT_TEXT_CHARS, MAX_DOCUMENT_TEXT_CHARS))
        return {
            **_document_view(document),
            "content": text[:cap],
            "character_count": len(text),
            "truncated": len(text) > cap,
        }

    @server.tool(name="documents.ingest_text")
    async def ingest_text(
        filename: str,
        content: str,
        workspace_id: str,
        rag_mode: str = "simple",
    ) -> dict[str, Any]:
        """Store text as a document and index it. Not available to the chat agent."""
        await require_workspace(database, workspace_id)
        try:
            document_id = await app.ingestion.index_text(
                workspace_id,
                filename,
                content,
                rag_mode=rag_mode or "simple",
            )
        except ValueError as exc:
            raise ToolError(str(exc)) from exc
        return await document_status(document_id)

    @server.tool(name="documents.delete")
    async def delete_document(document_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Delete a document, its chunks and its graph nodes. Requires confirmed=true."""
        if not confirmed:
            raise ToolError("Document deletion requires confirmed=true")
        try:
            await app.ingestion.delete_document(document_id, confirmed=True)
        except PermissionError as exc:
            raise ToolError(str(exc)) from exc
        return {"deleted": True}

    # --- retrieval router -------------------------------------------------

    @server.tool(
        name="rag.auto.search",
        description=(
            "Search the workspace, letting Private AI choose the retrieval strategy.\n\n"
            "Routes by the shape of the question: an exhaustive summary request goes to "
            "rag.summary, a question about how entities relate goes to rag.graph, a "
            "quoted phrase or an identifier goes to rag.keyword, anything else goes to "
            "rag.hybrid. Use this when you have no clear reason to pick one of the rag.* "
            "servers yourself; the answer says which strategy ran and why.\n\n"
            f"{UNTRUSTED_FRAMING}"
        ),
    )
    async def auto_search(query: str, workspace_id: str, limit: int = 5) -> dict[str, Any]:
        await require_workspace(database, workspace_id)
        strategy = app.strategies.get("auto")
        documents = await strategy.retrieve(
            query,
            workspace_id=workspace_id,
            limit=max(1, min(limit, 20)),
        )
        payload = results_payload(query, "auto", documents)
        if documents:
            metadata = documents[0].metadata
            payload["routed_to"] = str(metadata.get("strategy") or "")
            payload["routing_reason"] = str(metadata.get("routing_reason") or "")
        else:
            payload["routed_to"], payload["routing_reason"] = strategy.classify(query)
        return payload

    # --- memory -----------------------------------------------------------

    @server.tool(name="memory.list")
    async def list_memory(include_disabled: bool = False) -> list[dict[str, Any]]:
        """List what the user has explicitly asked Private AI to remember about them."""
        memories = await repositories.list_memories(
            database,
            include_disabled=include_disabled,
        )
        return [memory.model_dump(mode="json") for memory in memories]

    @server.tool(name="memory.search")
    async def search_memory(query: str, limit: int = 5) -> list[dict[str, Any]]:
        """Search enabled memories semantically, with a keyword fallback."""
        user_id = await repositories.active_profile_id_async(database)
        documents = await app.memory.search(query, user_id=user_id, limit=max(1, min(limit, 20)))
        return [{"content": document.page_content, **document.metadata} for document in documents]

    @server.tool(name="memory.remember")
    async def remember(
        content: str,
        memory_type: str = "fact",
        source: str = "mcp",
    ) -> dict[str, Any]:
        """Store a user-approved preference, fact or episode. Not available to the agent."""
        try:
            normalized = MemoryType(memory_type).value
        except ValueError as exc:
            raise ToolError(f"Unsupported memory type: {memory_type}") from exc
        user_id = await repositories.active_profile_id_async(database)
        try:
            memory_id = await app.memory.remember(
                content,
                memory_type=normalized,
                source=source,
                user_id=user_id,
            )
        except ValueError as exc:
            raise ToolError(str(exc)) from exc
        return {"id": memory_id, "type": normalized, "content": content.strip()}

    @server.tool(name="memory.update")
    async def update_memory(
        memory_id: str,
        content: str,
        enabled: bool = True,
    ) -> dict[str, Any]:
        """Rewrite or disable one memory. Not available to the chat agent."""
        if not await database.fetch_one_async(
            "SELECT id FROM memories WHERE id = ?",
            (memory_id,),
        ):
            raise ToolError("Memory not found")
        try:
            await app.memory.update(memory_id, content, enabled)
        except ValueError as exc:
            raise ToolError(str(exc)) from exc
        return {"id": memory_id, "content": content.strip(), "enabled": enabled}

    @server.tool(name="memory.forget")
    async def forget_memory(memory_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Permanently forget one memory. Requires confirmed=true."""
        if not confirmed:
            raise ToolError("Forgetting memory requires confirmed=true")
        if not await database.fetch_one_async(
            "SELECT id FROM memories WHERE id = ?",
            (memory_id,),
        ):
            raise ToolError("Memory not found")
        await app.memory.forget(memory_id, confirmed=True)
        return {"forgotten": True}

    # --- models -----------------------------------------------------------

    async def model_inventory() -> list[dict[str, Any]]:
        """Installed models plus the bundled ASR runtime, which no provider lists.

        Speech recognition runs on a local binary rather than through the model provider,
        so it has no entry to list. The UI's model screen still has to show it, hence the
        synthetic row.
        """
        from private_ai.asr.service import ASR_MODEL_NAME
        from private_ai.llm import ProviderUnavailable

        try:
            inventory = [model.model_dump(mode="json") for model in await app.models.list_models()]
        except ProviderUnavailable:
            inventory = []

        # The configured embedding model is written without a tag; the provider reports it
        # with one. Repointing the default at the canonical spelling stops every later
        # lookup from missing.
        configured_embedding = settings.embedding_model.removesuffix(":latest")
        canonical_embedding = next(
            (
                str(model["name"])
                for model in inventory
                if model["model_type"] == "embedding"
                and str(model["name"]).removesuffix(":latest") == configured_embedding
            ),
            settings.embedding_model,
        )
        if canonical_embedding != settings.embedding_model:
            await database.execute_async(
                "UPDATE model_defaults SET model_name = ? "
                "WHERE task = 'embedding' AND model_name = ?",
                (canonical_embedding, settings.embedding_model),
            )

        asr_status = app.asr.status()
        inventory.append(
            {
                "name": ASR_MODEL_NAME,
                "model_type": "asr",
                "state": "loaded" if asr_status["native_model_loaded"] else "unloaded",
                "size_bytes": asr_status["size_bytes"],
                "vram_bytes": (
                    settings.asr_vram_reservation_bytes if asr_status["native_model_loaded"] else 0
                ),
                "quantization": "Q4_K_M",
                "capabilities": ["transcription", "streaming", settings.asr_language],
                "runtime": "transcribe.cpp",
                "sha256": asr_status["sha256"],
                "available": asr_status["available"],
            }
        )
        defaults = await repositories.get_model_defaults(database)
        for model in inventory:
            model["default_for"] = [
                task for task, model_name in defaults.items() if model_name == model["name"]
            ]
        return inventory

    @server.tool(name="models.list")
    async def list_models() -> list[dict[str, Any]]:
        """List installed local models with their state, size and capabilities."""
        return await model_inventory()

    @server.tool(name="models.status")
    async def model_status(name: str) -> dict[str, Any]:
        """Lifecycle, disk, VRAM, runtime and integrity for one installed model."""
        model = next((item for item in await model_inventory() if item["name"] == name), None)
        if not model:
            raise ToolError("Model not found")
        return model

    @server.tool(name="models.capabilities")
    async def model_capabilities(name: str) -> dict[str, Any]:
        """What tasks one installed model can do."""
        model = await model_status(name)
        return {
            "name": name,
            "model_type": model["model_type"],
            "capabilities": model.get("capabilities", []),
            "runtime": model.get("runtime", ""),
        }

    @server.tool(name="models.select_default")
    async def select_default_model(task: str, name: str) -> dict[str, str]:
        """Set the default model for chat, embedding, vision or ASR. Not for the agent."""
        model = await model_status(name)
        expected = {"chat": "language", "embedding": "embedding", "asr": "asr"}.get(task)
        if expected and model["model_type"] != expected:
            raise ToolError(f"Task {task} requires a {expected} model")
        try:
            await repositories.set_model_default(database, task, name)
        except ValueError as exc:
            raise ToolError(str(exc)) from exc
        return {"task": task, "model": name}

    # --- this machine -----------------------------------------------------

    @server.tool(name="system.info")
    def system_info() -> dict[str, Any]:
        """This machine: OS, CPU, memory, GPU budget and free disk. Read locally."""
        return machine_snapshot(settings, app.gpu_leases)

    @server.tool(name="system.time")
    def system_time() -> dict[str, Any]:
        """The current local and UTC date and time. Call this before assuming today."""
        return time_snapshot()

    # --- files ------------------------------------------------------------

    @server.tool(name="files.allowed")
    def allowed_files() -> dict[str, Any]:
        """Which folders are readable right now, and where each permission came from."""
        return {
            "configured_roots": [str(root) for root in app.files.roots],
            "granted": [grant.public() for grant in app.files.grants()],
        }

    async def _authorize(path: Path, action: str, ctx: Context) -> None:
        """Open a path only if the user allowed it, asking them right now if they have not.

        The ask rides MCP elicitation, which the 2026-07-28 revision carries inside the
        tool result, so it works even on the stateless HTTP transport. A client too old to
        answer gets a refusal naming the setting that pre-approves a folder instead.
        """
        if app.files.is_protected(path):
            raise ToolError("This path holds the MCP token and is never readable")
        if app.files.is_allowed(path):
            return
        try:
            outcome = await ctx.elicit(
                message=(
                    f"Private AI muốn {action}: {path}\n"
                    "Đường dẫn này chưa được cấp quyền. Cho phép?"
                ),
                schema=FileAccessDecision,
            )
        except Exception as exc:  # noqa: BLE001 - any transport failure means nobody was asked
            raise ToolError(
                f"Cannot reach {path}: it is outside the allowed folders and this client "
                "cannot ask for permission. Pre-approve a folder with PRIVATE_AI_FILE_ROOTS."
            ) from exc
        if not isinstance(outcome, AcceptedElicitation) or not outcome.data.allow:
            raise ToolError(f"The user declined access to {path}")
        if outcome.data.remember_folder:
            app.files.remember(path)

    @server.tool(name="files.list")
    async def list_files(path: str, ctx: Context, limit: int = 50) -> dict[str, Any]:
        """List one folder on this machine, asking the user when it is not allowed yet."""
        try:
            target = app.files.resolve(path)
            await _authorize(target, "liệt kê thư mục", ctx)
            return app.files.list_directory(target, limit)
        except FileAccessError as exc:
            raise ToolError(str(exc)) from exc

    @server.tool(
        name="files.read",
        description=(
            "Read one text file on this machine, asking the user when it is not allowed "
            "yet.\n\nBinary files are refused rather than returned as replacement "
            "characters, and a long file comes back truncated with the flag set.\n\n"
            f"{UNTRUSTED_FRAMING}"
        ),
    )
    async def read_file(path: str, ctx: Context, max_bytes: int = 0) -> dict[str, Any]:
        try:
            target = app.files.resolve(path)
            await _authorize(target, "đọc tệp", ctx)
            return app.files.read_file(target, max_bytes)
        except FileAccessError as exc:
            raise ToolError(str(exc)) from exc

    return server


def run() -> None:
    stdio_entry(create_server)
