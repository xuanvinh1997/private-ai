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
from mcp.server.mcpserver import MCPServer
from mcp.server.transport_security import TransportSecuritySettings

from private_ai_api.config import Settings, get_settings
from private_ai_api.database import Database
from private_ai_api.schemas import MemoryType
from private_ai_api.services.asr import ASR_MODEL_NAME, AsrService
from private_ai_api.services.document_processor import DocumentProcessor
from private_ai_api.services.gpu_lease import GpuLeaseManager, InsufficientVram
from private_ai_api.services.graph_store import GraphStore
from private_ai_api.services.memory_service import MemoryService
from private_ai_api.services.ollama import OllamaClient, OllamaUnavailable


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


def create_mcp_server(settings: Settings | None = None) -> MCPServer:
    configured = settings or get_settings()
    configured.data_dir.mkdir(parents=True, exist_ok=True)
    configured.documents_dir.mkdir(parents=True, exist_ok=True)
    database = Database(configured.database_path)
    # Either process can win the race to migrate, so both have to clear the orphaned files.
    for purged in database.initialize():
        shutil.rmtree(Path(purged).parent, ignore_errors=True)
    gpu_leases = GpuLeaseManager(configured.gpu_capacity_bytes)
    ollama = OllamaClient(
        configured.ollama_url,
        configured.request_timeout_seconds,
        gpu_leases=gpu_leases,
        model_overhead_ratio=configured.gpu_model_overhead_ratio,
    )
    graph = GraphStore(
        database,
        url=configured.neo4j_url,
        user=configured.neo4j_user,
        password=configured.resolved_neo4j_password(),
        neo4j_database=configured.neo4j_database,
        enabled=configured.neo4j_enabled,
    )
    documents = DocumentProcessor(
        database,
        ollama,
        embedding_model=configured.embedding_model,
        embedding_enabled=configured.embedding_enabled,
        graph_store=graph,
        graph_entity_model=configured.graph_entity_model,
    )
    memories = MemoryService(
        database,
        ollama,
        graph,
        embedding_model=configured.embedding_model,
        embedding_enabled=configured.embedding_enabled,
    )
    asr = AsrService(
        data_dir=configured.asr_dir,
        executable=configured.asr_executable,
        model_path=configured.asr_model or configured.default_asr_model_path,
        language=configured.asr_language,
        ffmpeg_executable=configured.ffmpeg_executable,
        enabled=configured.asr_enabled,
        gpu_leases=gpu_leases,
        vram_reservation_bytes=configured.asr_vram_reservation_bytes,
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
            raise ValueError("Workspace not found; call workspaces.list for valid IDs")
        return workspace_id

    def _workspace_document_ids(workspace_id: str) -> set[str]:
        return {
            str(row["id"])
            for row in database.fetch_all(
                "SELECT id FROM documents WHERE workspace_id = ?",
                (workspace_id,),
            )
        }

    def _workspace_entity_keys(workspace_id: str) -> set[str]:
        return {
            str(row["key"])
            for row in database.fetch_all(
                """
                SELECT DISTINCT e.key FROM chunk_entities AS e
                JOIN documents AS d ON d.id = e.document_id
                WHERE d.workspace_id = ?
                """,
                (workspace_id,),
            )
        }

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
            raise ValueError("Document not found")
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

    @server.tool(name="documents.get_chunk")
    def get_document_chunk(chunk_id: str) -> dict[str, Any]:
        """Get an exact chunk and its source metadata by chunk ID."""
        chunk = database.fetch_one(
            """
            SELECT c.id AS chunk_id, c.document_id, d.workspace_id, c.chunk_index, c.content,
                   c.section_id, c.section_title, c.section_level, c.page_number,
                   c.embedding_model, d.filename
            FROM document_chunks AS c
            JOIN documents AS d ON d.id = c.document_id
            WHERE c.id = ?
            """,
            (chunk_id,),
        )
        if not chunk:
            raise ValueError("Document chunk not found")
        return chunk

    @server.tool(name="documents.ingest_text")
    async def ingest_text(filename: str, content: str, workspace_id: str) -> dict[str, Any]:
        """Store, chunk, and index user-provided Markdown or plain text in one workspace."""
        if not content.strip():
            raise ValueError("Document content cannot be empty")
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
        now = datetime.now(UTC).isoformat()
        database.execute(
            """
            INSERT INTO documents(
                id, workspace_id, filename, media_type, sha256, byte_size, status, source_path,
                extracted_text, error, created_at, updated_at
            ) VALUES (?, ?, ?, 'text/markdown', ?, ?, 'ready', ?, ?, NULL, ?, ?)
            """,
            (
                document_id,
                workspace_id,
                safe_name,
                digest,
                len(content.encode("utf-8")),
                str(target_path),
                content,
                now,
                now,
            ),
        )
        documents.index_text(document_id, content)
        await documents.embed_document(document_id)
        return document_status(document_id)

    @server.tool(name="documents.delete")
    async def delete_document(document_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Delete a local document only after explicit confirmation."""
        if not confirmed:
            raise ValueError("Document deletion requires confirmed=true")
        if not await documents.delete(document_id):
            raise ValueError("Document not found")
        return {"deleted": True}

    @server.tool(name="graph.search")
    async def search_graph(
        query: str,
        workspace_id: str,
        limit: int = 5,
    ) -> list[dict[str, object]]:
        """Search Neo4j indexes within one workspace; return an empty list when offline."""
        _require_workspace(workspace_id)
        if not configured.embedding_enabled:
            return []
        try:
            vectors = await ollama.embed(configured.embedding_model, [query])
        except (InsufficientVram, OllamaUnavailable):
            return []
        # Neo4j stores every workspace's graph in one place, so scope on the way out.
        owned = _workspace_document_ids(workspace_id)
        return [
            record
            for record in await graph.search(query, vectors[0], max(1, min(limit, 20)))
            if str(record.get("document_id")) in owned
        ]

    @server.tool(name="graph.find_entity")
    async def find_graph_entity(
        query: str,
        workspace_id: str,
        limit: int = 20,
    ) -> list[dict[str, object]]:
        """Find graph entities mentioned by one workspace's documents."""
        _require_workspace(workspace_id)
        keys = _workspace_entity_keys(workspace_id)
        return [
            record
            for record in await graph.find_entities(query, max(1, min(limit, 100)))
            if str(record.get("key")) in keys
        ]

    @server.tool(name="graph.neighborhood")
    async def graph_neighborhood(
        entity_key: str,
        workspace_id: str,
        limit: int = 30,
    ) -> dict[str, object]:
        """Expand a known entity by up to two hops, citing only this workspace's chunks."""
        _require_workspace(workspace_id)
        if entity_key not in _workspace_entity_keys(workspace_id):
            return {"entity": None, "neighbors": [], "chunks": []}
        owned = _workspace_document_ids(workspace_id)
        result = await graph.neighborhood(entity_key, max(1, min(limit, 100)))
        chunks = result.get("chunks")
        if isinstance(chunks, list):
            result["chunks"] = [
                chunk
                for chunk in chunks
                if isinstance(chunk, dict) and str(chunk.get("document_id")) in owned
            ]
        return result

    @server.tool(name="graph.find_relationships")
    async def find_graph_relationships(
        workspace_id: str,
        source_key: str = "",
        target_key: str = "",
        limit: int = 50,
    ) -> list[dict[str, object]]:
        """Read one workspace's entity relationships; arbitrary Cypher is not exposed."""
        _require_workspace(workspace_id)
        owned = _workspace_document_ids(workspace_id)
        return [
            record
            for record in await graph.relationships(
                source_key,
                target_key,
                max(1, min(limit, 200)),
            )
            if str(record.get("document_id")) in owned
        ]

    @server.tool(name="graph.answer")
    async def graph_answer(query: str, workspace_id: str, limit: int = 5) -> dict[str, object]:
        """Return an evidence bundle suitable for a grounded answer."""
        _require_workspace(workspace_id)
        search_results = await documents.search(
            query,
            max(1, min(limit, 20)),
            workspace_id=workspace_id,
        )
        entities = await graph.find_entities(query, 10)
        return {"query": query, "sources": search_results, "entities": entities}

    @server.tool(name="memory.list")
    def list_memory(include_disabled: bool = False) -> list[dict[str, Any]]:
        """List user-approved personal memory entries."""
        predicate = "user_id = 'local-user'" + ("" if include_disabled else " AND enabled = 1")
        return database.fetch_all(
            "SELECT id, user_id, type, content, source, confidence, enabled, "
            "created_at, updated_at, expires_at FROM memories "
            f"WHERE {predicate} ORDER BY updated_at DESC"  # noqa: S608
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
            raise ValueError("Memory content cannot be empty")
        memory_id = str(uuid4())
        now = datetime.now(UTC).isoformat()
        database.execute(
            """
            INSERT INTO memories(
                id, user_id, type, content, source, confidence, enabled,
                created_at, updated_at, expires_at
            ) VALUES (?, 'local-user', ?, ?, ?, 1, 1, ?, ?, NULL)
            """,
            (memory_id, normalized_type, content.strip(), source, now, now),
        )
        await memories.sync_memory(memory_id)
        return database.fetch_one(
            "SELECT id, user_id, type, content, source, confidence, enabled, "
            "created_at, updated_at, expires_at FROM memories WHERE id = ?",
            (memory_id,),
        ) or {}

    @server.tool(name="memory.search")
    async def search_memory(query: str, limit: int = 5) -> list[dict[str, Any]]:
        """Search enabled personal memory entries semantically with local fallback."""
        return await memories.search(
            query,
            user_id="local-user",
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
            raise ValueError("Memory content cannot be empty")
        existing = database.fetch_one("SELECT id FROM memories WHERE id = ?", (memory_id,))
        if not existing:
            raise ValueError("Memory not found")
        database.execute(
            "UPDATE memories SET content = ?, enabled = ?, updated_at = ?, "
            "embedding_json = NULL, embedding_model = NULL WHERE id = ?",
            (content.strip(), int(enabled), datetime.now(UTC).isoformat(), memory_id),
        )
        await memories.sync_memory(memory_id)
        return database.fetch_one(
            "SELECT id, user_id, type, content, source, confidence, enabled, "
            "created_at, updated_at, expires_at FROM memories WHERE id = ?",
            (memory_id,),
        ) or {}

    @server.tool(name="memory.forget")
    async def forget_memory(memory_id: str, confirmed: bool = False) -> dict[str, bool]:
        """Permanently forget a memory only after explicit confirmation."""
        if not confirmed:
            raise ValueError("Forgetting memory requires confirmed=true")
        existing = database.fetch_one("SELECT id FROM memories WHERE id = ?", (memory_id,))
        if not existing:
            raise ValueError("Memory not found")
        await memories.delete_memory(memory_id)
        return {"forgotten": True}

    async def model_inventory() -> list[dict[str, Any]]:
        try:
            inventory = [model.model_dump(mode="json") for model in await ollama.list_models()]
        except OllamaUnavailable:
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
            raise ValueError("Model not found")
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
            raise ValueError("Unsupported model task")
        model = await model_status(name)
        expected = {"chat": "language", "embedding": "embedding", "asr": "asr"}.get(task)
        if expected and model["model_type"] != expected:
            raise ValueError(f"Task {task} requires a {expected} model")
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
