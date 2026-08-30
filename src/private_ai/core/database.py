from __future__ import annotations

import asyncio
import json
import sqlite3
import threading
from collections.abc import Iterable
from contextlib import contextmanager
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

# The profile that owns rows written before profiles existed.
LEGACY_PROFILE_ID = "local-user"
ACTIVE_PROFILE_KEY = "active_profile_id"

SCHEMA = """
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    type TEXT NOT NULL CHECK(type IN ('preference', 'fact', 'episodic')),
    content TEXT NOT NULL,
    source TEXT NOT NULL,
    confidence REAL NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    expires_at TEXT,
    embedding_json TEXT,
    embedding_model TEXT
);

CREATE INDEX IF NOT EXISTS memories_user_enabled
ON memories(user_id, enabled, updated_at DESC);

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    media_type TEXT,
    sha256 TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    status TEXT NOT NULL,
    source_path TEXT NOT NULL,
    extracted_text TEXT,
    error TEXT,
    use_ocr INTEGER,
    index_mode TEXT NOT NULL DEFAULT 'simple' CHECK(index_mode IN ('simple', 'graph')),
    graph_model TEXT,
    indexed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(workspace_id, sha256)
);

CREATE INDEX IF NOT EXISTS documents_workspace_created
ON documents(workspace_id, created_at DESC);

CREATE TABLE IF NOT EXISTS document_sections (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    section_index INTEGER NOT NULL,
    title TEXT NOT NULL,
    level INTEGER NOT NULL DEFAULT 0,
    page_start INTEGER,
    page_end INTEGER,
    created_at TEXT NOT NULL,
    UNIQUE(document_id, section_index)
);

CREATE INDEX IF NOT EXISTS document_sections_document
ON document_sections(document_id, section_index);

CREATE TABLE IF NOT EXISTS document_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    section_id TEXT,
    section_title TEXT,
    section_level INTEGER NOT NULL DEFAULT 0,
    page_number INTEGER,
    embedding_json TEXT,
    embedding_vector BLOB,
    embedding_model TEXT,
    graph_model TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(document_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS document_chunks_document
ON document_chunks(document_id, chunk_index);

CREATE TABLE IF NOT EXISTS chunk_entities (
    chunk_id TEXT NOT NULL REFERENCES document_chunks(id) ON DELETE CASCADE,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    source_model TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(chunk_id, key)
);

CREATE INDEX IF NOT EXISTS chunk_entities_document
ON chunk_entities(document_id, chunk_id);

CREATE TABLE IF NOT EXISTS chunk_relations (
    chunk_id TEXT NOT NULL REFERENCES document_chunks(id) ON DELETE CASCADE,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    target_key TEXT NOT NULL,
    relation TEXT NOT NULL,
    source_model TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(chunk_id, source_key, target_key, relation)
);

CREATE INDEX IF NOT EXISTS chunk_relations_document
ON chunk_relations(document_id, chunk_id);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    document_id TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0,
    payload_json TEXT NOT NULL,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One row per document that some process is actively ingesting. The in-process asyncio
-- lock cannot span processes, and uvicorn --reload happily runs the old and the new worker
-- side by side, so the claim has to live in the database. A claim whose heartbeat has gone
-- quiet belongs to a process that died and may be taken over.
CREATE TABLE IF NOT EXISTS document_claims (
    document_id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    claimed_at TEXT NOT NULL,
    renewed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_defaults (
    task TEXT PRIMARY KEY CHECK(task IN ('chat', 'embedding', 'vision', 'asr')),
    model_name TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS model_events (
    id TEXT PRIMARY KEY,
    model_name TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed')),
    detail TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS model_events_created
ON model_events(created_at DESC);

CREATE TABLE IF NOT EXISTS ai_providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('ollama', 'openai')),
    base_url TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_access_grants (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    recursive INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    model TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS conversations_workspace_updated
ON conversations(workspace_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS messages_conversation_created
ON messages(conversation_id, created_at ASC);

CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'builtin',
    path TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('builtin', 'stdio', 'http')),
    command TEXT NOT NULL DEFAULT '',
    args_json TEXT NOT NULL DEFAULT '[]',
    url TEXT NOT NULL DEFAULT '',
    headers_json TEXT NOT NULL DEFAULT '{}',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
    strategy TEXT NOT NULL DEFAULT '',
    skills_json TEXT NOT NULL DEFAULT '[]',
    tools_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'running',
    error TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS agent_runs_conversation
ON agent_runs(conversation_id, started_at DESC);
"""


class Database:
    def __init__(self, path: Path) -> None:
        self.path = path
        # One connection per thread, kept open. FastAPI runs sync endpoints on an anyio
        # worker thread and the ingestion process has threads of its own, so reconnecting
        # per query paid the open/PRAGMA cost thousands of times over a single ingestion.
        self._local = threading.local()

    def initialize(self) -> list[str]:
        """Create or migrate the schema, returning document paths the caller must delete."""
        self.path.parent.mkdir(parents=True, exist_ok=True)
        purged = self._purge_workspaceless_documents()
        with self.connection() as connection:
            connection.executescript(SCHEMA)
            self._ensure_column(connection, "document_chunks", "embedding_json", "TEXT")
            # Ranking used to json.loads every stored embedding and score it in pure Python.
            # Packed float32 lets one numpy call replace that whole loop.
            self._ensure_column(connection, "document_chunks", "embedding_vector", "BLOB")
            self._ensure_column(connection, "document_chunks", "embedding_model", "TEXT")
            self._ensure_column(connection, "document_chunks", "graph_model", "TEXT")
            self._ensure_column(connection, "document_chunks", "section_id", "TEXT")
            self._ensure_column(connection, "document_chunks", "section_title", "TEXT")
            self._ensure_column(
                connection,
                "document_chunks",
                "section_level",
                "INTEGER NOT NULL DEFAULT 0",
            )
            self._ensure_column(connection, "document_chunks", "page_number", "INTEGER")
            self._ensure_column(connection, "documents", "use_ocr", "INTEGER")
            self._ensure_column(connection, "documents", "indexed_at", "TEXT")
            index_mode_added = self._ensure_column(
                connection,
                "documents",
                "index_mode",
                "TEXT NOT NULL DEFAULT 'simple' CHECK(index_mode IN ('simple', 'graph'))",
            )
            self._ensure_column(connection, "documents", "graph_model", "TEXT")
            # Before index modes existed every completed document went through LightRAG's
            # graph pipeline. Preserve that fact instead of relabelling old data as simple.
            if index_mode_added:
                connection.execute(
                    "UPDATE documents SET index_mode = 'graph' WHERE indexed_at IS NOT NULL"
                )
            self._ensure_column(connection, "jobs", "document_id", "TEXT")
            self._ensure_column(connection, "memories", "embedding_json", "TEXT")
            self._ensure_column(connection, "memories", "embedding_model", "TEXT")
            connection.execute(
                "CREATE INDEX IF NOT EXISTS jobs_document_updated "
                "ON jobs(document_id, updated_at DESC)"
            )
            self._backfill_document_sections(connection)
            self._seed_workspaces(connection)
            self._seed_profile(connection)
        return purged

    def _purge_workspaceless_documents(self) -> list[str]:
        """Drop a pre-workspace ``documents`` table so the schema can recreate it scoped.

        Documents used to be a single global library shared by every workspace. There is
        no correct workspace to attribute those rows to, so the library is wiped once and
        the on-disk paths are handed back for the caller to remove.
        """
        connection = sqlite3.connect(self.path, timeout=30)
        connection.row_factory = sqlite3.Row
        try:
            connection.execute("PRAGMA foreign_keys=OFF")
            columns = {row[1] for row in connection.execute("PRAGMA table_info(documents)")}
            if not columns or "workspace_id" in columns:
                return []
            paths = [
                str(row["source_path"])
                for row in connection.execute("SELECT source_path FROM documents")
            ]
            for table in (
                "chunk_relations",
                "chunk_entities",
                "document_chunks",
                "document_sections",
            ):
                connection.execute(f"DELETE FROM {table}")  # noqa: S608
            connection.execute("DROP TABLE documents")
            connection.commit()
            return paths
        except Exception:
            connection.rollback()
            raise
        finally:
            connection.close()

    @staticmethod
    def _ensure_column(
        connection: sqlite3.Connection,
        table: str,
        column: str,
        declaration: str,
    ) -> bool:
        columns = {row[1] for row in connection.execute(f"PRAGMA table_info({table})")}  # noqa: S608
        if column not in columns:
            connection.execute(f"ALTER TABLE {table} ADD COLUMN {column} {declaration}")  # noqa: S608
            return True
        return False

    @staticmethod
    def _backfill_document_sections(connection: sqlite3.Connection) -> None:
        now = datetime.now(UTC).isoformat()
        document_ids = connection.execute(
            "SELECT DISTINCT document_id FROM document_chunks "
            "WHERE section_id IS NULL OR section_id = ''"
        ).fetchall()
        for (document_id,) in document_ids:
            section_id = f"{document_id}:section:0"
            connection.execute(
                """
                INSERT OR IGNORE INTO document_sections(
                    id, document_id, section_index, title, level,
                    page_start, page_end, created_at
                ) VALUES (?, ?, 0, 'Nội dung', 0, NULL, NULL, ?)
                """,
                (section_id, document_id, now),
            )
            connection.execute(
                """
                UPDATE document_chunks
                SET section_id = ?, section_title = COALESCE(section_title, 'Nội dung'),
                    section_level = COALESCE(section_level, 0)
                WHERE document_id = ? AND (section_id IS NULL OR section_id = '')
                """,
                (section_id, document_id),
            )

    @staticmethod
    def _seed_profile(connection: sqlite3.Connection) -> None:
        """Give every database exactly one profile to start from.

        Its id is the ``local-user`` that memories were written against before profiles
        existed, so those rows stay attached to the person who wrote them. The name is
        left empty on purpose: that is what the web app reads as "not introduced yet"
        and answers with the onboarding prompt.
        """
        existing = connection.execute("SELECT COUNT(*) FROM profiles").fetchone()[0]
        if existing:
            return
        now = datetime.now(UTC).isoformat()
        connection.execute(
            "INSERT INTO profiles(id, display_name, created_at, updated_at) VALUES (?, '', ?, ?)",
            (LEGACY_PROFILE_ID, now, now),
        )
        connection.execute(
            """
            INSERT INTO app_state(key, value) VALUES (?, ?)
            ON CONFLICT(key) DO NOTHING
            """,
            (ACTIVE_PROFILE_KEY, LEGACY_PROFILE_ID),
        )

    @staticmethod
    def _seed_workspaces(connection: sqlite3.Connection) -> None:
        """Insert the starter workspaces once, on the very first run of a database.

        Re-seeding on every start would resurrect workspaces the user deleted, so the
        run is recorded in ``app_state``. A database that predates this marker but
        already holds workspaces is treated as seeded.
        """
        marked = connection.execute(
            "SELECT 1 FROM app_state WHERE key = 'workspaces_seeded'"
        ).fetchone()
        if marked:
            return
        now = datetime.now(UTC).isoformat()
        connection.execute(
            "INSERT INTO app_state(key, value) VALUES ('workspaces_seeded', ?)",
            (now,),
        )
        existing = connection.execute("SELECT COUNT(*) FROM workspaces").fetchone()[0]
        if existing:
            return
        defaults = (
            (
                "personal",
                "Trợ lý cá nhân",
                "Hỏi đáp và công việc hằng ngày",
                now,
                now,
            ),
            (
                "research",
                "Nghiên cứu tài liệu",
                "Tìm kiếm trong thư viện riêng",
                now,
                now,
            ),
            (
                "private-ai",
                "Dự án Private AI",
                "Kiến trúc và ghi chú kỹ thuật",
                now,
                now,
            ),
        )
        connection.executemany(
            """
            INSERT OR IGNORE INTO workspaces(id, name, description, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            """,
            defaults,
        )

    def _thread_connection(self) -> sqlite3.Connection:
        connection = getattr(self._local, "connection", None)
        if connection is not None:
            return connection
        connection = sqlite3.connect(self.path, timeout=30)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA journal_mode=WAL")
        connection.execute("PRAGMA foreign_keys=ON")
        # WAL already survives a process crash at NORMAL; only an OS-level crash can lose
        # the last commits, which is the right trade for a local index we can rebuild.
        connection.execute("PRAGMA synchronous=NORMAL")
        # The API and the ingestion worker write to the same file, so a writer that arrives
        # mid-transaction has to wait rather than fail the request outright.
        connection.execute("PRAGMA busy_timeout=30000")
        self._local.connection = connection
        self._local.depth = 0
        return connection

    @contextmanager
    def connection(self) -> Iterable[sqlite3.Connection]:
        connection = self._thread_connection()
        # Nesting has to reuse the outer transaction: committing here would publish the
        # caller's half-written batch, and closing would strand it.
        if getattr(self._local, "depth", 0):
            self._local.depth += 1
            try:
                yield connection
            finally:
                self._local.depth -= 1
            return
        self._local.depth = 1
        try:
            yield connection
            connection.commit()
        except Exception:
            connection.rollback()
            raise
        finally:
            self._local.depth = 0

    def close(self) -> None:
        """Drop this thread's connection. Other threads keep theirs."""
        connection = getattr(self._local, "connection", None)
        if connection is None:
            return
        self._local.connection = None
        self._local.depth = 0
        connection.close()

    def fetch_all(self, query: str, parameters: tuple[Any, ...] = ()) -> list[dict[str, Any]]:
        with self.connection() as connection:
            rows = connection.execute(query, parameters).fetchall()
        return [dict(row) for row in rows]

    def fetch_one(self, query: str, parameters: tuple[Any, ...] = ()) -> dict[str, Any] | None:
        with self.connection() as connection:
            row = connection.execute(query, parameters).fetchone()
        return dict(row) if row else None

    def execute(self, query: str, parameters: tuple[Any, ...] = ()) -> None:
        with self.connection() as connection:
            connection.execute(query, parameters)

    def execute_many(self, query: str, parameters: Iterable[tuple[Any, ...]]) -> None:
        with self.connection() as connection:
            connection.executemany(query, parameters)

    # SQLite is synchronous, so every call above blocks whichever thread runs it. Inside an
    # `async def` that thread is the event loop, which then serves nobody until the query
    # returns. These wrappers are what async handlers should reach for instead.
    async def fetch_all_async(
        self,
        query: str,
        parameters: tuple[Any, ...] = (),
    ) -> list[dict[str, Any]]:
        return await asyncio.to_thread(self.fetch_all, query, parameters)

    async def fetch_one_async(
        self,
        query: str,
        parameters: tuple[Any, ...] = (),
    ) -> dict[str, Any] | None:
        return await asyncio.to_thread(self.fetch_one, query, parameters)

    async def execute_async(self, query: str, parameters: tuple[Any, ...] = ()) -> None:
        await asyncio.to_thread(self.execute, query, parameters)

    def upsert_job(self, job: dict[str, Any]) -> None:
        self.execute(
            """
            INSERT INTO jobs(
                id, document_id, kind, status, progress, payload_json, error, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                document_id=excluded.document_id,
                status=excluded.status,
                progress=excluded.progress,
                payload_json=excluded.payload_json,
                error=excluded.error,
                updated_at=excluded.updated_at
            """,
            (
                job["id"],
                job.get("document_id"),
                job["kind"],
                job["status"],
                job["progress"],
                json.dumps(job.get("payload", {})),
                job.get("error"),
                job["created_at"],
                job["updated_at"],
            ),
        )
