"""Schema creation, migration and the two seeds that must never run twice."""

from __future__ import annotations

import sqlite3
from datetime import UTC, datetime
from pathlib import Path

from private_ai.core.database import ACTIVE_PROFILE_KEY, LEGACY_PROFILE_ID, Database


def _tables(database: Database) -> set[str]:
    rows = database.fetch_all("SELECT name FROM sqlite_master WHERE type = 'table'")
    return {str(row["name"]) for row in rows}


def test_initialize_creates_every_table_the_application_reads(database: Database) -> None:
    expected = {
        "workspaces",
        "conversations",
        "messages",
        "documents",
        "document_sections",
        "document_chunks",
        "document_claims",
        "memories",
        "jobs",
        "model_defaults",
        "model_events",
        "ai_providers",
        "profiles",
        "app_state",
        "file_access_grants",
        "skills",
        "mcp_servers",
        "agent_runs",
    }
    assert expected <= _tables(database)


def test_initialize_is_idempotent(database: Database) -> None:
    before = _tables(database)
    assert database.initialize() == []
    assert _tables(database) == before


def test_starter_workspaces_are_seeded_once_and_never_resurrected(
    database: Database,
) -> None:
    """Re-seeding on every start would bring back workspaces the user deleted."""
    seeded = {str(row["id"]) for row in database.fetch_all("SELECT id FROM workspaces")}
    assert seeded == {"personal", "research", "private-ai"}

    database.execute("DELETE FROM workspaces WHERE id = 'research'")
    database.initialize()

    remaining = {str(row["id"]) for row in database.fetch_all("SELECT id FROM workspaces")}
    assert "research" not in remaining


def test_the_single_seeded_profile_owns_pre_profile_rows(database: Database) -> None:
    profiles = database.fetch_all("SELECT id, display_name FROM profiles")
    assert [row["id"] for row in profiles] == [LEGACY_PROFILE_ID]
    # Empty on purpose: that is what the UI reads as "not introduced yet".
    assert profiles[0]["display_name"] == ""
    active = database.fetch_one("SELECT value FROM app_state WHERE key = ?", (ACTIVE_PROFILE_KEY,))
    assert active is not None
    assert active["value"] == LEGACY_PROFILE_ID


def test_nested_connections_share_one_transaction(database: Database) -> None:
    """An inner ``with`` must not commit the outer caller's half-written batch."""
    try:
        with database.connection() as outer:
            outer.execute(
                "INSERT INTO app_state(key, value) VALUES ('outer', '1')",
            )
            with database.connection() as inner:
                inner.execute("INSERT INTO app_state(key, value) VALUES ('inner', '1')")
            raise RuntimeError("caller failed after the inner block returned")
    except RuntimeError:
        pass

    assert database.fetch_one("SELECT value FROM app_state WHERE key = 'inner'") is None
    assert database.fetch_one("SELECT value FROM app_state WHERE key = 'outer'") is None


async def test_async_wrappers_read_and_write_the_same_rows(database: Database) -> None:
    await database.execute_async("INSERT INTO app_state(key, value) VALUES ('k', 'v')")
    row = await database.fetch_one_async("SELECT value FROM app_state WHERE key = 'k'")
    assert row == {"value": "v"}
    rows = await database.fetch_all_async("SELECT key FROM app_state WHERE key = 'k'")
    assert rows == [{"key": "k"}]


def test_a_pre_workspace_document_table_is_purged_and_its_files_reported(
    tmp_path: Path,
) -> None:
    """Documents used to be global; there is no correct workspace to attribute them to."""
    path = tmp_path / "legacy.db"
    # Build today's schema, then put ``documents`` back the way it looked before it was
    # scoped: everything else the purge touches is exactly as it was on that install.
    seeded = Database(path)
    seeded.initialize()
    seeded.close()
    connection = sqlite3.connect(path)
    connection.executescript(
        """
        DROP TABLE documents;
        CREATE TABLE documents (
            id TEXT PRIMARY KEY,
            filename TEXT NOT NULL,
            source_path TEXT NOT NULL
        );
        """
    )
    connection.execute(
        "INSERT INTO documents(id, filename, source_path) VALUES ('d1', 'a.pdf', ?)",
        (str(tmp_path / "d1" / "a.pdf"),),
    )
    connection.commit()
    connection.close()

    database = Database(path)
    try:
        purged = database.initialize()
        assert purged == [str(tmp_path / "d1" / "a.pdf")]
        columns = {
            str(row["name"])
            for row in database.fetch_all("SELECT name FROM pragma_table_info('documents')")
        }
        assert "workspace_id" in columns
    finally:
        database.close()


def test_documents_indexed_before_index_modes_existed_are_kept_as_graph(
    tmp_path: Path,
) -> None:
    """Relabelling them 'simple' would claim a vector index that was never built."""
    path = tmp_path / "old.db"
    now = datetime.now(UTC).isoformat()
    connection = sqlite3.connect(path)
    connection.executescript(
        """
        CREATE TABLE workspaces (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE documents (
            id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, filename TEXT NOT NULL,
            media_type TEXT, sha256 TEXT NOT NULL, byte_size INTEGER NOT NULL,
            status TEXT NOT NULL, source_path TEXT NOT NULL, extracted_text TEXT,
            error TEXT, indexed_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        """
    )
    connection.execute(
        "INSERT INTO workspaces VALUES ('w', 'W', '', ?, ?)",
        (now, now),
    )
    connection.execute(
        "INSERT INTO documents VALUES ('d', 'w', 'f.pdf', NULL, 'sha', 1, 'ready', '/f.pdf',"
        " 'text', NULL, ?, ?, ?)",
        (now, now, now),
    )
    connection.commit()
    connection.close()

    database = Database(path)
    try:
        database.initialize()
        row = database.fetch_one("SELECT index_mode FROM documents WHERE id = 'd'")
        assert row == {"index_mode": "graph"}
    finally:
        database.close()
