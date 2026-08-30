"""Data access for everything the UI lists, opens, edits and deletes.

These are the queries the old HTTP routers used to run, lifted out of them. There is no
request object left to carry a 404 or a 409, so the two failure modes the routers
encoded in status codes are exceptions here: ``NotFound`` for a missing row, and a plain
``ValueError`` when a destructive call arrives without ``confirmed=True``. Every
destructive entry point keeps that flag — the UI, the agent and an external MCP client
all go through these functions, and none of them may delete by accident.
"""

from __future__ import annotations

import json
from datetime import UTC, datetime
from typing import Any
from uuid import uuid4

from private_ai.core.database import ACTIVE_PROFILE_KEY, Database
from private_ai.core.schemas import (
    ConversationCreate,
    ConversationDetail,
    ConversationRecord,
    McpServerRecord,
    MemoryRecord,
    MessageRecord,
    ProfileRecord,
    SkillRecord,
    WorkspaceCreate,
    WorkspaceRecord,
    WorkspaceUpdate,
)

DEFAULT_CONVERSATION_TITLE = "Cuộc trò chuyện mới"
MODEL_TASKS = ("chat", "embedding", "vision", "asr")
MAX_DOCUMENT_PAGE = 100
MAX_MODEL_EVENTS = 200


class NotFound(LookupError):
    """The row the caller named does not exist."""


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _require(row: dict[str, Any] | None, message: str) -> dict[str, Any]:
    if not row:
        raise NotFound(message)
    return row


def _confirm(confirmed: bool, message: str) -> None:
    if not confirmed:
        raise ValueError(message)


# --- workspaces -----------------------------------------------------------

_WORKSPACE_COLUMNS = """
    SELECT w.*, COUNT(c.id) AS conversation_count
    FROM workspaces w
    LEFT JOIN conversations c ON c.workspace_id = w.id
"""


async def list_workspaces(database: Database) -> list[WorkspaceRecord]:
    rows = await database.fetch_all_async(
        f"{_WORKSPACE_COLUMNS} GROUP BY w.id ORDER BY w.updated_at DESC"
    )
    return [WorkspaceRecord(**row) for row in rows]


async def get_workspace(database: Database, workspace_id: str) -> WorkspaceRecord:
    row = await database.fetch_one_async(
        f"{_WORKSPACE_COLUMNS} WHERE w.id = ? GROUP BY w.id",
        (workspace_id,),
    )
    return WorkspaceRecord(**_require(row, f"Workspace {workspace_id} not found"))


async def create_workspace(database: Database, payload: WorkspaceCreate) -> WorkspaceRecord:
    workspace_id = str(uuid4())
    now = _now()
    await database.execute_async(
        """
        INSERT INTO workspaces(id, name, description, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?)
        """,
        (workspace_id, payload.name, payload.description, now, now),
    )
    return await get_workspace(database, workspace_id)


async def update_workspace(
    database: Database,
    workspace_id: str,
    payload: WorkspaceUpdate,
) -> WorkspaceRecord:
    existing = _require(
        await database.fetch_one_async("SELECT * FROM workspaces WHERE id = ?", (workspace_id,)),
        f"Workspace {workspace_id} not found",
    )
    await database.execute_async(
        "UPDATE workspaces SET name = ?, description = ?, updated_at = ? WHERE id = ?",
        (
            payload.name if payload.name is not None else existing["name"],
            payload.description if payload.description is not None else existing["description"],
            _now(),
            workspace_id,
        ),
    )
    return await get_workspace(database, workspace_id)


async def delete_workspace(
    database: Database,
    workspace_id: str,
    *,
    confirmed: bool = False,
) -> list[str]:
    """Drop the workspace and report the documents the caller still has to clean up.

    Conversations cascade in SQLite, but a document also owns files on disk and nodes in
    the knowledge index. Those live outside this module, so the ids go back to the caller
    rather than the row being quietly orphaned.
    """
    _confirm(confirmed, "Workspace deletion requires confirmation")
    _require(
        await database.fetch_one_async("SELECT id FROM workspaces WHERE id = ?", (workspace_id,)),
        f"Workspace {workspace_id} not found",
    )
    documents = await database.fetch_all_async(
        "SELECT id FROM documents WHERE workspace_id = ?",
        (workspace_id,),
    )
    await database.execute_async("DELETE FROM workspaces WHERE id = ?", (workspace_id,))
    return [str(document["id"]) for document in documents]


# --- conversations --------------------------------------------------------

_CONVERSATION_COLUMNS = """
    SELECT c.*, COUNT(m.id) AS message_count
    FROM conversations c
    LEFT JOIN messages m ON m.conversation_id = c.id
"""


async def list_conversations(database: Database, workspace_id: str) -> list[ConversationRecord]:
    rows = await database.fetch_all_async(
        f"{_CONVERSATION_COLUMNS} WHERE c.workspace_id = ? "
        "GROUP BY c.id ORDER BY c.updated_at DESC",
        (workspace_id,),
    )
    return [ConversationRecord(**row) for row in rows]


async def _conversation_row(database: Database, conversation_id: str) -> dict[str, Any]:
    row = await database.fetch_one_async(
        f"{_CONVERSATION_COLUMNS} WHERE c.id = ? GROUP BY c.id",
        (conversation_id,),
    )
    return _require(row, f"Conversation {conversation_id} not found")


async def create_conversation(
    database: Database,
    workspace_id: str,
    payload: ConversationCreate | None = None,
) -> ConversationRecord:
    _require(
        await database.fetch_one_async("SELECT id FROM workspaces WHERE id = ?", (workspace_id,)),
        f"Workspace {workspace_id} not found",
    )
    payload = payload or ConversationCreate()
    conversation_id = str(uuid4())
    now = _now()
    await database.execute_async(
        """
        INSERT INTO conversations(id, workspace_id, title, model, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (conversation_id, workspace_id, payload.title, payload.model, now, now),
    )
    return ConversationRecord(**await _conversation_row(database, conversation_id))


async def get_conversation(database: Database, conversation_id: str) -> ConversationDetail:
    conversation = await _conversation_row(database, conversation_id)
    messages = await database.fetch_all_async(
        "SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at ASC",
        (conversation_id,),
    )
    return ConversationDetail(
        **conversation,
        messages=[MessageRecord(**message) for message in messages],
    )


async def delete_conversation(
    database: Database,
    conversation_id: str,
    *,
    confirmed: bool = False,
) -> None:
    _confirm(confirmed, "Conversation deletion requires confirmation")
    await _conversation_row(database, conversation_id)
    await database.execute_async("DELETE FROM conversations WHERE id = ?", (conversation_id,))


async def append_message(
    database: Database,
    conversation_id: str,
    role: str,
    content: str,
) -> MessageRecord:
    if role not in {"user", "assistant", "system"}:
        raise ValueError(f"Unsupported message role: {role}")
    message_id = str(uuid4())
    now = _now()
    await database.execute_async(
        """
        INSERT INTO messages(id, conversation_id, role, content, created_at)
        VALUES (?, ?, ?, ?, ?)
        """,
        (message_id, conversation_id, role, content, now),
    )
    return MessageRecord(
        id=message_id,
        conversation_id=conversation_id,
        role=role,
        content=content,
        created_at=now,
    )


async def touch_conversation(
    database: Database,
    conversation_id: str,
    *,
    model: str | None = None,
) -> None:
    """Bump the conversation and its workspace, so both sort to the top of their list."""
    now = _now()
    if model:
        await database.execute_async(
            "UPDATE conversations SET model = ?, updated_at = ? WHERE id = ?",
            (model, now, conversation_id),
        )
    else:
        await database.execute_async(
            "UPDATE conversations SET updated_at = ? WHERE id = ?",
            (now, conversation_id),
        )
    await database.execute_async(
        """
        UPDATE workspaces SET updated_at = ?
        WHERE id = (SELECT workspace_id FROM conversations WHERE id = ?)
        """,
        (now, conversation_id),
    )


async def autotitle_conversation(
    database: Database,
    conversation_id: str,
    seed: str,
) -> str:
    """Name an untitled conversation after its first question, and leave named ones alone."""
    row = await database.fetch_one_async(
        "SELECT title FROM conversations WHERE id = ?",
        (conversation_id,),
    )
    current = str(_require(row, f"Conversation {conversation_id} not found")["title"])
    if current != DEFAULT_CONVERSATION_TITLE:
        return current
    title = seed.strip().replace("\n", " ")[:80] or current
    await database.execute_async(
        "UPDATE conversations SET title = ?, updated_at = ? WHERE id = ?",
        (title, _now(), conversation_id),
    )
    return title


# --- profiles -------------------------------------------------------------

_PROFILE_COLUMNS = """
    SELECT p.*, COUNT(m.id) AS memory_count
    FROM profiles p
    LEFT JOIN memories m ON m.user_id = p.id
"""


def active_profile_id(database: Database) -> str:
    """The profile every unattributed read and write belongs to.

    Synchronous on purpose: the ingestion worker and the MCP servers reach for it from
    plain functions, and it is a single indexed read. Use ``active_profile_id_async``
    from the UI's event loop.

    Falls back to the oldest profile so a stale or missing pointer cannot leave the app
    without an identity; the pointer is repaired on the next activation.
    """
    stored = database.fetch_one(
        "SELECT value FROM app_state WHERE key = ?",
        (ACTIVE_PROFILE_KEY,),
    )
    if stored:
        candidate = str(stored["value"])
        if database.fetch_one("SELECT 1 FROM profiles WHERE id = ?", (candidate,)):
            return candidate
    row = database.fetch_one("SELECT id FROM profiles ORDER BY created_at ASC LIMIT 1")
    return str(row["id"]) if row else ""


async def active_profile_id_async(database: Database) -> str:
    stored = await database.fetch_one_async(
        "SELECT value FROM app_state WHERE key = ?",
        (ACTIVE_PROFILE_KEY,),
    )
    if stored:
        candidate = str(stored["value"])
        if await database.fetch_one_async("SELECT 1 FROM profiles WHERE id = ?", (candidate,)):
            return candidate
    row = await database.fetch_one_async("SELECT id FROM profiles ORDER BY created_at ASC LIMIT 1")
    return str(row["id"]) if row else ""


async def list_profiles(database: Database) -> list[ProfileRecord]:
    active_id = await active_profile_id_async(database)
    rows = await database.fetch_all_async(
        f"{_PROFILE_COLUMNS} GROUP BY p.id ORDER BY p.created_at ASC"
    )
    return [ProfileRecord(**row, active=str(row["id"]) == active_id) for row in rows]


async def get_profile(database: Database, profile_id: str) -> ProfileRecord:
    row = _require(
        await database.fetch_one_async(
            f"{_PROFILE_COLUMNS} WHERE p.id = ? GROUP BY p.id",
            (profile_id,),
        ),
        f"Profile {profile_id} not found",
    )
    active_id = await active_profile_id_async(database)
    return ProfileRecord(**row, active=str(row["id"]) == active_id)


async def create_profile(database: Database, display_name: str = "") -> ProfileRecord:
    """Add a profile and switch to it, which is the only reason to add one."""
    profile_id = str(uuid4())
    now = _now()
    await database.execute_async(
        "INSERT INTO profiles(id, display_name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        (profile_id, display_name.strip(), now, now),
    )
    await activate_profile(database, profile_id)
    return await get_profile(database, profile_id)


async def rename_profile(database: Database, profile_id: str, display_name: str) -> ProfileRecord:
    _require(
        await database.fetch_one_async("SELECT id FROM profiles WHERE id = ?", (profile_id,)),
        f"Profile {profile_id} not found",
    )
    await database.execute_async(
        "UPDATE profiles SET display_name = ?, updated_at = ? WHERE id = ?",
        (display_name.strip(), _now(), profile_id),
    )
    return await get_profile(database, profile_id)


async def activate_profile(database: Database, profile_id: str) -> ProfileRecord:
    _require(
        await database.fetch_one_async("SELECT id FROM profiles WHERE id = ?", (profile_id,)),
        f"Profile {profile_id} not found",
    )
    await database.execute_async(
        """
        INSERT INTO app_state(key, value) VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        """,
        (ACTIVE_PROFILE_KEY, profile_id),
    )
    return await get_profile(database, profile_id)


async def delete_profile(
    database: Database,
    profile_id: str,
    *,
    confirmed: bool = False,
) -> None:
    """Remove a profile and everything it remembers.

    Workspaces, conversations and documents are shared by everyone on this machine, so
    they are left alone; only the memories written under this profile go with it.
    """
    _confirm(confirmed, "Profile deletion requires confirmation")
    _require(
        await database.fetch_one_async("SELECT id FROM profiles WHERE id = ?", (profile_id,)),
        f"Profile {profile_id} not found",
    )
    remaining = await database.fetch_all_async(
        "SELECT id FROM profiles WHERE id != ? ORDER BY created_at ASC",
        (profile_id,),
    )
    if not remaining:
        raise ValueError("The last profile cannot be deleted")
    await database.execute_async("DELETE FROM memories WHERE user_id = ?", (profile_id,))
    await database.execute_async("DELETE FROM profiles WHERE id = ?", (profile_id,))
    if await active_profile_id_async(database) == profile_id:
        await activate_profile(database, str(remaining[0]["id"]))


# --- memory ---------------------------------------------------------------


async def list_memories(
    database: Database,
    user_id: str = "",
    *,
    include_disabled: bool = False,
) -> list[MemoryRecord]:
    """Read only. Writing a memory goes through ``MemoryStore``, which also embeds it."""
    owner = user_id or await active_profile_id_async(database)
    predicate = "user_id = ?" if include_disabled else "user_id = ? AND enabled = 1"
    rows = await database.fetch_all_async(
        f"SELECT * FROM memories WHERE {predicate} ORDER BY updated_at DESC",  # noqa: S608
        (owner,),
    )
    return [MemoryRecord(**{**row, "enabled": bool(row["enabled"])}) for row in rows]


# --- documents ------------------------------------------------------------


def _ingestion_view(job: dict[str, Any], document: dict[str, Any]) -> dict[str, Any]:
    try:
        payload = json.loads(str(job.get("payload_json") or "{}"))
    except json.JSONDecodeError:
        payload = {}
    return {
        "id": job["id"],
        "status": job["status"],
        "progress": float(job["progress"]),
        "step": payload.get("step", "queued"),
        "detail": payload.get("detail", ""),
        "index_mode": payload.get("index_mode", document.get("index_mode", "simple")),
        "graph_model": payload.get("graph_model", document.get("graph_model") or ""),
        "engine": payload.get("engine", "lightrag"),
        "embedded_vectors": int(payload.get("embedded_vectors", 0) or 0),
        "estimated_chunks": int(payload.get("estimated_chunks", 0) or 0),
        "vectors_per_second": float(payload.get("vectors_per_second", 0) or 0),
        "elapsed_seconds": float(payload.get("elapsed_seconds", 0) or 0),
        "error": job.get("error"),
        "updated_at": job["updated_at"],
    }


async def _with_ingestion(
    database: Database,
    documents: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Attach each document's most recent job, which is what the progress row renders."""
    if not documents:
        return []
    document_ids = [str(document["id"]) for document in documents]
    placeholders = ",".join("?" for _ in document_ids)
    jobs = await database.fetch_all_async(
        f"SELECT * FROM jobs WHERE document_id IN ({placeholders}) "  # noqa: S608
        "ORDER BY updated_at DESC",
        tuple(document_ids),
    )
    latest: dict[str, dict[str, Any]] = {}
    for job in jobs:
        document_id = str(job.get("document_id") or "")
        if document_id and document_id not in latest:
            latest[document_id] = job

    decorated: list[dict[str, Any]] = []
    for document in documents:
        item = dict(document)
        job = latest.get(str(item["id"]))
        if job:
            item["ingestion"] = _ingestion_view(job, item)
        decorated.append(item)
    return decorated


async def list_documents(
    database: Database,
    workspace_id: str,
    *,
    q: str = "",
    status: str = "",
    limit: int = 20,
    offset: int = 0,
) -> dict[str, Any]:
    _require(
        await database.fetch_one_async("SELECT id FROM workspaces WHERE id = ?", (workspace_id,)),
        f"Workspace {workspace_id} not found",
    )
    page_size = max(1, min(limit, MAX_DOCUMENT_PAGE))
    start = max(0, offset)

    clauses = ["workspace_id = ?"]
    parameters: list[Any] = [workspace_id]
    if q.strip():
        clauses.append("filename LIKE ?")
        parameters.append(f"%{q.strip()}%")
    if status:
        clauses.append("status = ?")
        parameters.append(status)
    where = " AND ".join(clauses)

    counted = await database.fetch_one_async(
        f"SELECT COUNT(*) AS total FROM documents WHERE {where}",  # noqa: S608
        tuple(parameters),
    )
    # Totals for the whole workspace stay stable so the header does not jump while filtering.
    summary = await database.fetch_one_async(
        """
        SELECT COUNT(*) AS total,
               COALESCE(SUM(byte_size), 0) AS byte_size,
               COALESCE(SUM(status IN ('queued', 'processing')), 0) AS pending,
               COALESCE(SUM(status = 'ready' AND extracted_text IS NOT NULL
                            AND indexed_at IS NULL), 0) AS indexing,
               COALESCE(SUM(status IN ('failed', 'needs_ocr')), 0) AS failed
        FROM documents WHERE workspace_id = ?
        """,
        (workspace_id,),
    )
    rows = await database.fetch_all_async(
        f"SELECT * FROM documents WHERE {where} "  # noqa: S608
        "ORDER BY created_at DESC LIMIT ? OFFSET ?",
        (*parameters, page_size, start),
    )
    return {
        "items": await _with_ingestion(database, rows),
        "total": int(counted["total"]) if counted else 0,
        "limit": page_size,
        "offset": start,
        "summary": dict(summary or {}),
    }


async def get_document(database: Database, document_id: str) -> dict[str, Any]:
    row = _require(
        await database.fetch_one_async("SELECT * FROM documents WHERE id = ?", (document_id,)),
        f"Document {document_id} not found",
    )
    return (await _with_ingestion(database, [row]))[0]


async def delete_document_row(
    database: Database,
    document_id: str,
    *,
    confirmed: bool = False,
) -> str:
    """Remove the row and hand back its source path.

    Chunks, sections and entities cascade. The file on disk and the graph nodes do not,
    and neither belongs to this module, so the caller finishes the job.
    """
    _confirm(confirmed, "Document deletion requires confirmation")
    row = _require(
        await database.fetch_one_async(
            "SELECT source_path FROM documents WHERE id = ?",
            (document_id,),
        ),
        f"Document {document_id} not found",
    )
    await database.execute_async("DELETE FROM documents WHERE id = ?", (document_id,))
    return str(row["source_path"])


async def queue_document(
    database: Database,
    document_id: str,
    *,
    use_ocr: bool | None = None,
) -> None:
    """Put a document back in the queue, optionally flipping its OCR choice for good.

    The row's own ``status`` is the queue: the worker polls for it and claims the
    document in its own process, so there is nothing else to schedule here.
    """
    _require(
        await database.fetch_one_async("SELECT id FROM documents WHERE id = ?", (document_id,)),
        f"Document {document_id} not found",
    )
    if use_ocr is not None:
        await database.execute_async(
            "UPDATE documents SET use_ocr = ? WHERE id = ?",
            (int(use_ocr), document_id),
        )
    await database.execute_async(
        "UPDATE documents SET status = 'queued', error = NULL, indexed_at = NULL WHERE id = ?",
        (document_id,),
    )


# --- models ---------------------------------------------------------------


async def get_model_defaults(database: Database) -> dict[str, str]:
    rows = await database.fetch_all_async("SELECT task, model_name FROM model_defaults")
    return {str(row["task"]): str(row["model_name"]) for row in rows}


async def set_model_default(database: Database, task: str, model: str) -> None:
    if task not in MODEL_TASKS:
        raise ValueError(f"Unsupported model task: {task}")
    await database.execute_async(
        """
        INSERT INTO model_defaults(task, model_name, updated_at) VALUES (?, ?, ?)
        ON CONFLICT(task) DO UPDATE SET model_name=excluded.model_name,
                                        updated_at=excluded.updated_at
        """,
        (task, model, _now()),
    )


async def record_model_event(
    database: Database,
    model_name: str,
    action: str,
    status: str,
    detail: str | None = None,
) -> None:
    if status not in {"completed", "failed"}:
        raise ValueError(f"Unsupported model event status: {status}")
    await database.execute_async(
        """
        INSERT INTO model_events(id, model_name, action, status, detail, created_at)
        VALUES (?, ?, ?, ?, ?, ?)
        """,
        (str(uuid4()), model_name, action, status, detail, _now()),
    )


async def list_model_events(database: Database, limit: int = 50) -> list[dict[str, Any]]:
    return await database.fetch_all_async(
        "SELECT * FROM model_events ORDER BY created_at DESC LIMIT ?",
        (max(1, min(limit, MAX_MODEL_EVENTS)),),
    )


# --- capability inventory -------------------------------------------------


async def list_skills(database: Database) -> list[SkillRecord]:
    rows = await database.fetch_all_async("SELECT * FROM skills ORDER BY name ASC")
    return [SkillRecord(**{**row, "enabled": bool(row["enabled"])}) for row in rows]


async def set_skill_enabled(database: Database, skill_id: str, enabled: bool) -> None:
    await database.execute_async(
        "UPDATE skills SET enabled = ?, updated_at = ? WHERE id = ?",
        (int(enabled), _now(), skill_id),
    )


async def list_mcp_servers(database: Database) -> list[McpServerRecord]:
    rows = await database.fetch_all_async("SELECT * FROM mcp_servers ORDER BY name ASC")
    return [
        McpServerRecord(
            id=str(row["id"]),
            name=str(row["name"]),
            kind=str(row["kind"]),
            command=str(row["command"] or ""),
            args=_json_list(row["args_json"]),
            url=str(row["url"] or ""),
            headers=_json_dict(row["headers_json"]),
            enabled=bool(row["enabled"]),
            created_at=row["created_at"],
            updated_at=row["updated_at"],
        )
        for row in rows
    ]


def _json_list(raw: Any) -> list[str]:
    try:
        parsed = json.loads(str(raw or "[]"))
    except json.JSONDecodeError:
        return []
    return [str(item) for item in parsed] if isinstance(parsed, list) else []


def _json_dict(raw: Any) -> dict[str, str]:
    try:
        parsed = json.loads(str(raw or "{}"))
    except json.JSONDecodeError:
        return {}
    return {str(k): str(v) for k, v in parsed.items()} if isinstance(parsed, dict) else {}


async def set_mcp_server_enabled(database: Database, server_id: str, enabled: bool) -> None:
    await database.execute_async(
        "UPDATE mcp_servers SET enabled = ?, updated_at = ? WHERE id = ?",
        (int(enabled), _now(), server_id),
    )
