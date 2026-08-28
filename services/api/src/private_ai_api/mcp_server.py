from __future__ import annotations

import hashlib
import hmac
import os
import secrets
import shutil
from contextlib import suppress
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from uuid import uuid4

from mcp.server.auth.provider import AccessToken
from mcp.server.auth.settings import AuthSettings
from mcp.server.elicitation import AcceptedElicitation
from mcp.server.mcpserver import Context, MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from mcp.server.transport_security import TransportSecuritySettings
from pydantic import BaseModel, Field

from private_ai_api.config import Settings, get_settings
from private_ai_api.database import Database
from private_ai_api.routers.profiles import active_profile_id
from private_ai_api.schemas import MemoryType
from private_ai_api.services.app_preferences import read_app_preferences, read_web_search_config
from private_ai_api.services.asr import ASR_MODEL_NAME, AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.file_access import FileAccessError, FileAccessService
from private_ai_api.services.gpu_lease import GpuLeaseManager
from private_ai_api.services.lightrag_store import LightRagStore, default_model
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient
from private_ai_api.services.provider import ProviderUnavailable
from private_ai_api.services.provider_registry import ProviderRegistry, ProviderRouter
from private_ai_api.services.system_info import machine_snapshot, time_snapshot
from private_ai_api.services.web_search import WebSearchService, WebSearchUnavailable


class FileAccessDecision(BaseModel):
    """What the user is asked when a tool reaches for a path they have not allowed yet."""

    allow: bool = Field(description="Cho phép Private AI đọc đường dẫn này")
    remember_folder: bool = Field(
        default=False,
        description="Nhớ thư mục này để lần sau không phải hỏi lại",
    )


def _safe_filename(value: str) -> str:
    name = Path(value).name.strip().replace("\x00", "")
    return name or "note.md"


def _load_or_create_token(settings: Settings) -> str:
    if settings.mcp_token_path.is_file():
        token = settings.mcp_token_path.read_text(encoding="utf-8").strip()
        if token:
            return token
    token = secrets.token_urlsafe(32)
    settings.mcp_token_path.write_text(token, encoding="utf-8")
    with suppress(OSError):
        os.chmod(settings.mcp_token_path, 0o600)
    return token


class StaticTokenVerifier:
    def __init__(self, expected_token: str) -> None:
        self.expected_token = expected_token

    async def verify_token(self, token: str) -> AccessToken | None:
        if not hmac.compare_digest(token, self.expected_token):
            return None
        return AccessToken(
            token=token,
            client_id="private-ai-local-client",
            scopes=["private-ai"],
            subject="local-user",
        )


def create_mcp_server(
    settings: Settings | None = None,
    lightrag: LightRagStore | None = None,
    *,
    shared: Any = None,
) -> MCPServer:
    """Build the tool server, either standalone or on top of the API's live services.

    ``shared`` is the running ``AppServices``. Passing it matters: a second
    ``GpuLeaseManager`` would double-count every reservation, and a second ``LightRagStore``
    would open the same files twice. The standalone process passes nothing and builds its own.
    """
    configured = settings or get_settings()
    configured.data_dir.mkdir(parents=True, exist_ok=True)
    configured.documents_dir.mkdir(parents=True, exist_ok=True)
    database = shared.database if shared else Database(configured.database_path)
    if shared is None:
        # Either process can win the race to migrate, so both have to clear orphaned files.
        for purged in database.initialize():
            shutil.rmtree(Path(purged).parent, ignore_errors=True)
    app_preferences = read_app_preferences(database)
    gpu_leases = shared.gpu_leases if shared else GpuLeaseManager(configured.gpu_capacity_bytes)
    ollama = (
        shared.ollama
        if shared
        else OllamaClient(
            configured.ollama_url,
            configured.request_timeout_seconds,
                gpu_leases=gpu_leases,
            model_overhead_ratio=configured.gpu_model_overhead_ratio,
        )
    )
    ai = (
        shared.ai
        if shared
        else ProviderRouter(
            ProviderRegistry(
                database,
                ollama=ollama,
                ollama_url=configured.ollama_url,
                timeout=configured.request_timeout_seconds,
            )
        )
    )
    lightrag = lightrag or (shared.lightrag if shared else None) or LightRagStore(
        configured.data_dir,
        ai,
        embedding_model=configured.embedding_model,
        resolve_chat_model=lambda: default_model(database, "chat"),
        resolve_graph_model=lambda: (
            read_app_preferences(database).graph_model or default_model(database, "chat")
        ),
        enabled=configured.embedding_enabled,
        embedding_batch_size=app_preferences.embedding_batch_size,
        embedding_concurrency=app_preferences.embedding_concurrency,
    )
    documents = (
        shared.document_processor if shared else DocumentProcessor(database, lightrag, ai=ai)
    )
    memories = (
        shared.memory_service
        if shared
        else MemoryService(
            database,
            ai,
            embedding_model=configured.embedding_model,
            embedding_enabled=configured.embedding_enabled,
        )
    )
    files = FileAccessService(
        database,
        roots=configured.file_root_paths,
        protected=[configured.mcp_token_path],
        max_read_bytes=configured.file_read_max_bytes,
    )
    web_search = (
        shared.web_search
        if shared
        else WebSearchService(
            lambda: read_web_search_config(database),
            timeout=configured.web_search_timeout_seconds,
        )
    )
    asr = (
        shared.asr
        if shared
        else AsrService(
            data_dir=configured.asr_dir,
            executable=configured.asr_executable,
            model_path=configured.asr_model or configured.default_asr_model_path,
            language=configured.asr_language,
            ffmpeg_executable=configured.ffmpeg_executable,
            enabled=configured.asr_enabled,
        gpu_leases=gpu_leases,
            vram_reservation_bytes=configured.asr_vram_reservation_bytes,
        )
    )
    default_timestamp = datetime.now(UTC).isoformat()
    database.execute_many(
        "INSERT OR IGNORE INTO model_defaults(task, model_name, updated_at) VALUES (?, ?, ?)",
        (
            ("embedding", configured.embedding_model, default_timestamp),
            ("asr", ASR_MODEL_NAME, default_timestamp),
        ),
    )
    mcp_url = f"http://{configured.mcp_host}:{configured.mcp_port}/mcp"
    token_verifier = None
    auth = None
    if configured.mcp_require_auth:
        token_verifier = StaticTokenVerifier(_load_or_create_token(configured))
        auth = AuthSettings(
            issuer_url=f"http://{configured.mcp_host}:{configured.mcp_port}",
            resource_server_url=mcp_url,
            required_scopes=["private-ai"],
        )
    server = MCPServer(
        name="private-ai",
        title="Private AI local tools",
        description="Read and manage local documents and user-approved memory.",
        instructions=(
            "All data stays on this machine. Destructive tools require confirmed=true. "
            "Model deletion and arbitrary database queries are intentionally unavailable."
        ),
        token_verifier=token_verifier,
        auth=auth,
    )

    def _require_workspace(workspace_id: str) -> str:
        workspace = database.fetch_one(
            "SELECT id FROM workspaces WHERE id = ?",
            (workspace_id,),
        )
        if not workspace:
            raise ToolError("Workspace not found; call workspaces.list for valid IDs")
        return workspace_id

    @server.tool(name="workspaces.list")
    def list_workspaces() -> list[dict[str, Any]]:
        """List workspaces. Every document tool needs one of these IDs."""
        return database.fetch_all(
            """
            SELECT w.id, w.name, w.description, w.updated_at,
                   COUNT(d.id) AS document_count
            FROM workspaces AS w
            LEFT JOIN documents AS d ON d.workspace_id = w.id
            GROUP BY w.id
            ORDER BY w.updated_at DESC
            """
        )

    @server.tool(name="documents.list")
    def list_documents(workspace_id: str, limit: int = 50) -> list[dict[str, Any]]:
        """List one workspace's documents without returning their full extracted text."""
        _require_workspace(workspace_id)
        return database.fetch_all(
            """
            SELECT id, workspace_id, filename, media_type, byte_size, status, error,
                   created_at, updated_at
            FROM documents WHERE workspace_id = ? ORDER BY updated_at DESC LIMIT ?
            """,
            (workspace_id, max(1, min(limit, 200))),
        )

    @server.tool(name="documents.status")
    def document_status(document_id: str) -> dict[str, Any]:
        """Return ingestion and indexing status for one local document."""
        document = database.fetch_one(
            """
            SELECT id, workspace_id, filename, media_type, byte_size, status, error,
                   created_at, updated_at
            FROM documents WHERE id = ?
            """,
            (document_id,),
        )
        if not document:
            raise ToolError("Document not found")
        counts = database.fetch_one(
            """
            SELECT COUNT(*) AS chunks,
                   SUM(embedding_json IS NOT NULL) AS embedded_chunks
            FROM document_chunks WHERE document_id = ?
            """,
            (document_id,),
        )
        return {**document, **(counts or {"chunks": 0, "embedded_chunks": 0})}

    @server.tool(name="documents.search")
    async def search_documents(
        query: str,
        workspace_id: str,
        limit: int = 5,
    ) -> list[dict[str, object]]:
        """Hybrid keyword and semantic search over one workspace's document chunks."""
        _require_workspace(workspace_id)
        return await documents.search(query, max(1, min(limit, 20)), workspace_id=workspace_id)

    @server.tool(name="documents.ingest_text")
    async def ingest_text(
        filename: str,
        content: str,
        workspace_id: str,
        rag_mode: str = "",
        graph_model: str = "",
    ) -> dict[str, Any]:
        """Store and index text with fast vector RAG or the optional graph pipeline."""
        if not content.strip():
            raise ToolError("Document content cannot be empty")
        _require_workspace(workspace_id)
        safe_name = _safe_filename(filename)
        digest = hashlib.sha256(content.encode("utf-8")).hexdigest()
        duplicate = database.fetch_one(
            "SELECT * FROM documents WHERE workspace_id = ? AND sha256 = ?",
            (workspace_id, digest),
        )
        if duplicate:
            return duplicate
        document_id = str(uuid4())
        target_dir = configured.documents_dir / document_id
        target_dir.mkdir(parents=True, exist_ok=False)
        target_path = target_dir / safe_name
        target_path.write_text(content, encoding="utf-8")
        preferences = read_app_preferences(database)
        index_mode = rag_mode.strip() or preferences.rag_mode.value
        if index_mode not in {"simple", "graph"}:
            raise ToolError("rag_mode must be simple or graph")
        selected_graph_model = (
            graph_model.strip() or preferences.graph_model if index_mode == "graph" else ""
        )
        now = datetime.now(UTC).isoformat()
        database.execute(
            """
            INSERT INTO documents(
                id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
                extracted_text, index_mode, graph_model, error, created_at, updated_at
            ) VALUES (?, ?, ?, 'text/markdown', ?, ?, 'ready', ?, ?, ?, ?, NULL, ?, ?)
            """,
            (
                document_id,
                workspace_id,
                safe_name,
                digest,
                len(content.encode("utf-8")),
                str(target_path),
                content,
                index_mode,
                selected_graph_model or None,
                now,
                now,
            ),
        )
        await documents.index_document(document_id)
        return document_status(document_id)

    @server.tool(name="documents.delete")
    async def delete_document(document_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Delete a local document only after explicit confirmation."""
        if not confirmed:
            raise ToolError("Document deletion requires confirmed=true")
        if not await documents.delete(document_id):
            raise ToolError("Document not found")
        return {"deleted": True}

    @server.tool(name="graph.search")
    async def search_graph(
        query: str,
        workspace_id: str,
        limit: int = 5,
    ) -> list[dict[str, object]]:
        """Search one workspace's knowledge index; return an empty list when it is offline."""
        _require_workspace(workspace_id)
        return await lightrag.search(query, workspace_id, max(1, min(limit, 20)))

    @server.tool(name="graph.find_entity")
    async def find_graph_entity(
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, object]]:
        """Find graph entities mentioned by one workspace's documents."""
        _require_workspace(workspace_id)
        return await lightrag.find_entities(query, workspace_id, max(1, min(limit, 100)))

    @server.tool(name="graph.neighborhood")
    async def graph_neighborhood(
        entity_key: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        """Expand a known entity by up to two hops within one workspace."""
        _require_workspace(workspace_id)
        return await lightrag.neighborhood(entity_key, workspace_id, max(1, min(limit, 100)))

    @server.tool(name="graph.answer")
    async def graph_answer(query: str, workspace_id: str, limit: int = 5) -> dict[str, object]:
        """Return an evidence bundle suitable for a grounded answer."""
        _require_workspace(workspace_id)
        search_results = await documents.search(
            query,
            max(1, min(limit, 20)),
            workspace_id=workspace_id,
        )
        entities = await lightrag.find_entities(query, workspace_id, 10)
        return {"query": query, "sources": search_results, "entities": entities}

    @server.tool(name="web.search")
    async def search_web(query: str, limit: int = 5) -> dict[str, Any]:
        """Search the public web through the host the user configured in settings.

        This is the only tool here that sends anything off this machine, so it stays off
        until the user picks a search host. Results are untrusted third-party text.
        """
        try:
            found = await web_search.search(query, max(1, min(limit, 10)))
        except WebSearchUnavailable as exc:
            raise ToolError(str(exc)) from exc
        return found.public()

    @server.tool(name="system.info")
    def system_info() -> dict[str, Any]:
        """Report this machine: OS, CPU, memory, GPU budget and free disk. Stays local."""
        return machine_snapshot(configured, gpu_leases)

    @server.tool(name="system.time")
    def system_time() -> dict[str, Any]:
        """Return the current local and UTC date and time. Ask before assuming today's date."""
        return time_snapshot()

    @server.tool(name="files.allowed")
    def allowed_files() -> dict[str, Any]:
        """List the folders that are readable right now, and where each permission came from."""
        return {
            "configured_roots": [str(root) for root in files.roots],
            "granted": [grant.public() for grant in files.grants()],
        }

    async def _authorize(path: Path, action: str, ctx: Context) -> None:
        """Open a path only if the user allowed it, asking them right now when they have not.

        The ask rides MCP elicitation, which the 2026-07-28 revision carries inside the tool
        result, so it works on this stateless HTTP transport. A client too old to answer gets
        a refusal that names the setting which pre-approves a folder instead.
        """
        if files.is_protected(path):
            raise ToolError("This path holds the MCP token and is never readable")
        if files.is_allowed(path):
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
            files.remember(path)

    @server.tool(name="files.list")
    async def list_files(path: str, ctx: Context, limit: int = 50) -> dict[str, Any]:
        """List one folder on this machine, asking the user when it is not allowed yet."""
        try:
            target = files.resolve(path)
            await _authorize(target, "liệt kê thư mục", ctx)
            return files.list_directory(target, limit)
        except FileAccessError as exc:
            raise ToolError(str(exc)) from exc

    @server.tool(name="files.read")
    async def read_file(path: str, ctx: Context, max_bytes: int = 0) -> dict[str, Any]:
        """Read one text file on this machine, asking the user when it is not allowed yet.

        Binary files are refused rather than returned as replacement characters, and a long
        file comes back truncated with the flag set.
        """
        try:
            target = files.resolve(path)
            await _authorize(target, "đọc tệp", ctx)
            return files.read_file(target, max_bytes)
        except FileAccessError as exc:
            raise ToolError(str(exc)) from exc

    @server.tool(name="memory.list")
    def list_memory(include_disabled: bool = False) -> list[dict[str, Any]]:
        """List user-approved personal memory entries."""
        predicate = "user_id = ?" + ("" if include_disabled else " AND enabled = 1")
        return database.fetch_all(
            "SELECT id, user_id, type, content, source, confidence, enabled, "
            "created_at, updated_at, expires_at FROM memories "
            f"WHERE {predicate} ORDER BY updated_at DESC",  # noqa: S608
            (active_profile_id(database),),
        )

    @server.tool(name="memory.remember")
    async def remember(
        content: str,
        memory_type: str = "fact",
        source: str = "mcp",
    ) -> dict[str, Any]:
        """Store a user-approved preference, fact, or episodic memory."""
        normalized_type = MemoryType(memory_type).value
        if not content.strip():
            raise ToolError("Memory content cannot be empty")
        memory_id = str(uuid4())
        now = datetime.now(UTC).isoformat()
        database.execute(
            """
            INSERT INTO memories(
                id, user_id, type, content, source, confidence, enabled,
                created_at, updated_at, expires_at
            ) VALUES (?, ?, ?, ?, ?, 1, 1, ?, ?, NULL)
            """,
            (
                memory_id,
                active_profile_id(database),
                normalized_type,
                content.strip(),
                source,
                now,
                now,
            ),
        )
        await memories.sync_memory(memory_id)
        return (
            database.fetch_one(
                "SELECT id, user_id, type, content, source, confidence, enabled, "
                "created_at, updated_at, expires_at FROM memories WHERE id = ?",
                (memory_id,),
            )
            or {}
        )

    @server.tool(name="memory.search")
    async def search_memory(query: str, limit: int = 5) -> list[dict[str, Any]]:
        """Search enabled personal memory entries semantically with local fallback."""
        return await memories.search(
            query,
            user_id=active_profile_id(database),
            limit=max(1, min(limit, 20)),
        )

    @server.tool(name="memory.update")
    async def update_memory(
        memory_id: str,
        content: str,
        enabled: bool = True,
    ) -> dict[str, Any]:
        """Update content and enabled state for an existing memory entry."""
        if not content.strip():
            raise ToolError("Memory content cannot be empty")
        existing = database.fetch_one("SELECT id FROM memories WHERE id = ?", (memory_id,))
        if not existing:
            raise ToolError("Memory not found")
        database.execute(
            "UPDATE memories SET content = ?, enabled = ?, updated_at = ?, "
            "embedding_json = NULL, embedding_model = NULL WHERE id = ?",
            (content.strip(), int(enabled), datetime.now(UTC).isoformat(), memory_id),
        )
        await memories.sync_memory(memory_id)
        return (
            database.fetch_one(
                "SELECT id, user_id, type, content, source, confidence, enabled, "
                "created_at, updated_at, expires_at FROM memories WHERE id = ?",
                (memory_id,),
            )
            or {}
        )

    @server.tool(name="memory.forget")
    async def forget_memory(memory_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Permanently forget a memory only after explicit confirmation."""
        if not confirmed:
            raise ToolError("Forgetting memory requires confirmed=true")
        existing = database.fetch_one("SELECT id FROM memories WHERE id = ?", (memory_id,))
        if not existing:
            raise ToolError("Memory not found")
        await memories.delete_memory(memory_id)
        return {"forgotten": True}

    async def model_inventory() -> list[dict[str, Any]]:
        try:
            inventory = [model.model_dump(mode="json") for model in await ai.list_models()]
        except ProviderUnavailable:
            inventory = []
        configured_embedding = configured.embedding_model.removesuffix(":latest")
        canonical_embedding = next(
            (
                str(model["name"])
                for model in inventory
                if model["model_type"] == "embedding"
                and str(model["name"]).removesuffix(":latest") == configured_embedding
            ),
            configured.embedding_model,
        )
        database.execute(
            "UPDATE model_defaults SET model_name = ?, updated_at = ? "
            "WHERE task = 'embedding' AND model_name = ?",
            (canonical_embedding, datetime.now(UTC).isoformat(), configured.embedding_model),
        )
        asr_status = asr.status()
        inventory.append(
            {
                "name": ASR_MODEL_NAME,
                "model_type": "asr",
                "state": "loaded" if asr_status["native_model_loaded"] else "unloaded",
                "size_bytes": asr_status["size_bytes"],
                "vram_bytes": (
                    configured.asr_vram_reservation_bytes
                    if asr_status["native_model_loaded"]
                    else 0
                ),
                "quantization": "Q4_K_M",
                "capabilities": ["transcription", "streaming", configured.asr_language],
                "runtime": "transcribe.cpp",
                "sha256": asr_status["sha256"],
                "available": asr_status["available"],
            }
        )
        defaults = {
            str(row["task"]): str(row["model_name"])
            for row in database.fetch_all("SELECT task, model_name FROM model_defaults")
        }
        for model in inventory:
            model["default_for"] = [
                task for task, model_name in defaults.items() if model_name == model["name"]
            ]
        return inventory

    @server.tool(name="models.list")
    async def list_models() -> list[dict[str, Any]]:
        """List installed local models, state, size, and capabilities."""
        return await model_inventory()

    @server.tool(name="models.status")
    async def model_status(name: str) -> dict[str, Any]:
        """Return lifecycle, disk, VRAM, runtime, and integrity status for one model."""
        model = next((item for item in await model_inventory() if item["name"] == name), None)
        if not model:
            raise ToolError("Model not found")
        return model

    @server.tool(name="models.capabilities")
    async def model_capabilities(name: str) -> dict[str, Any]:
        """Return task capabilities for an installed model."""
        model = await model_status(name)
        return {
            "name": name,
            "model_type": model["model_type"],
            "capabilities": model["capabilities"],
            "runtime": model["runtime"],
        }

    @server.tool(name="models.select_default")
    async def select_default_model(task: str, name: str) -> dict[str, str]:
        """Select a default model for chat, embedding, vision, or ASR."""
        if task not in {"chat", "embedding", "vision", "asr"}:
            raise ToolError("Unsupported model task")
        model = await model_status(name)
        expected = {"chat": "language", "embedding": "embedding", "asr": "asr"}.get(task)
        if expected and model["model_type"] != expected:
            raise ToolError(f"Task {task} requires a {expected} model")
        database.execute(
            """
            INSERT INTO model_defaults(task, model_name, updated_at) VALUES (?, ?, ?)
            ON CONFLICT(task) DO UPDATE SET model_name=excluded.model_name,
                                            updated_at=excluded.updated_at
            """,
            (task, name, datetime.now(UTC).isoformat()),
        )
        return {"task": task, "model": name}

    return server


def run() -> None:
    settings = get_settings()
    server = create_mcp_server(settings)
    if settings.mcp_require_auth:
        print(f"MCP bearer token: {settings.mcp_token_path}")
    security = TransportSecuritySettings(
        enable_dns_rebinding_protection=True,
        allowed_hosts=[
            f"{settings.mcp_host}:{settings.mcp_port}",
            f"localhost:{settings.mcp_port}",
        ],
        allowed_origins=[
            f"http://{settings.mcp_host}:{settings.mcp_port}",
            f"http://localhost:{settings.mcp_port}",
        ],
    )
    with suppress(KeyboardInterrupt):
        server.run(
            "streamable-http",
            host=settings.mcp_host,
            port=settings.mcp_port,
            streamable_http_path="/mcp",
            stateless_http=True,
            json_response=True,
            transport_security=security,
        )


if __name__ == "__main__":
    run()
