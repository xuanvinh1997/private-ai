"""The queries the old HTTP routers ran, now called directly.

What used to be a 404 is ``NotFound`` and what used to be a 409 is ``ValueError``; the
tests below are mostly about those two, because they are the only place the routers'
behaviour could have been lost in the move.
"""

from __future__ import annotations

import pytest
from conftest import insert_document

from private_ai.core import repositories as repo
from private_ai.core.database import Database
from private_ai.core.schemas import ConversationCreate, WorkspaceCreate, WorkspaceUpdate


async def test_workspace_round_trip(database: Database) -> None:
    created = await repo.create_workspace(
        database,
        WorkspaceCreate(name="Nghiên cứu", description="ghi chú"),
    )
    assert created.name == "Nghiên cứu"
    assert created.conversation_count == 0

    updated = await repo.update_workspace(
        database,
        created.id,
        WorkspaceUpdate(description="đã đổi"),
    )
    # An omitted field keeps its old value rather than being blanked.
    assert (updated.name, updated.description) == ("Nghiên cứu", "đã đổi")

    listed = {workspace.id for workspace in await repo.list_workspaces(database)}
    assert created.id in listed


async def test_missing_rows_raise_not_found(database: Database) -> None:
    with pytest.raises(repo.NotFound):
        await repo.get_workspace(database, "nope")
    with pytest.raises(repo.NotFound):
        await repo.get_conversation(database, "nope")
    with pytest.raises(repo.NotFound):
        await repo.get_document(database, "nope")


async def test_every_destructive_call_refuses_without_confirmation(
    database: Database,
) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    conversation = await repo.create_conversation(database, workspace.id)
    document_id = insert_document(database, workspace.id, "a.txt", "nội dung")

    for call in (
        repo.delete_workspace(database, workspace.id),
        repo.delete_conversation(database, conversation.id),
        repo.delete_document_row(database, document_id),
        repo.delete_profile(database, "local-user"),
    ):
        with pytest.raises(ValueError):
            await call

    # Nothing was removed by the refusals.
    assert await repo.get_workspace(database, workspace.id)
    assert await repo.get_document(database, document_id)


async def test_deleting_a_workspace_hands_back_its_documents(database: Database) -> None:
    """Chunks cascade in SQLite; files on disk and graph nodes do not, so the ids escape."""
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    first = insert_document(database, workspace.id, "a.txt")
    second = insert_document(database, workspace.id, "b.txt")

    orphaned = await repo.delete_workspace(database, workspace.id, confirmed=True)

    assert sorted(orphaned) == sorted([first, second])
    with pytest.raises(repo.NotFound):
        await repo.get_workspace(database, workspace.id)


async def test_conversation_messages_come_back_in_order(database: Database) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    conversation = await repo.create_conversation(
        database,
        workspace.id,
        ConversationCreate(title=repo.DEFAULT_CONVERSATION_TITLE),
    )
    await repo.append_message(database, conversation.id, "user", "hỏi")
    await repo.append_message(database, conversation.id, "assistant", "đáp")

    detail = await repo.get_conversation(database, conversation.id)
    assert [(m.role, m.content) for m in detail.messages] == [
        ("user", "hỏi"),
        ("assistant", "đáp"),
    ]

    with pytest.raises(ValueError):
        await repo.append_message(database, conversation.id, "tool", "x")


async def test_autotitle_names_an_untitled_conversation_only_once(
    database: Database,
) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    conversation = await repo.create_conversation(database, workspace.id)
    assert conversation.title == repo.DEFAULT_CONVERSATION_TITLE

    first = await repo.autotitle_conversation(database, conversation.id, "Câu hỏi\nđầu tiên")
    assert first == "Câu hỏi đầu tiên"

    second = await repo.autotitle_conversation(database, conversation.id, "Câu hỏi khác")
    assert second == "Câu hỏi đầu tiên"


async def test_document_listing_pages_filters_and_keeps_a_stable_summary(
    database: Database,
) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    insert_document(database, workspace.id, "bao-cao.pdf", status="ready")
    insert_document(database, workspace.id, "ghi-chu.txt", status="ready")
    insert_document(database, workspace.id, "hong.pdf", status="failed")

    filtered = await repo.list_documents(database, workspace.id, q="bao", limit=10)
    assert [item["filename"] for item in filtered["items"]] == ["bao-cao.pdf"]
    assert filtered["total"] == 1
    # The header counts the whole workspace, so it does not jump while filtering.
    assert filtered["summary"]["total"] == 3
    assert filtered["summary"]["failed"] == 1

    by_status = await repo.list_documents(database, workspace.id, status="failed")
    assert [item["filename"] for item in by_status["items"]] == ["hong.pdf"]

    paged = await repo.list_documents(database, workspace.id, limit=2, offset=2)
    assert len(paged["items"]) == 1
    assert paged["total"] == 3


async def test_document_listing_caps_the_page_size(database: Database) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    page = await repo.list_documents(database, workspace.id, limit=10_000)
    assert page["limit"] == repo.MAX_DOCUMENT_PAGE


async def test_queue_document_clears_the_previous_failure(database: Database) -> None:
    workspace = await repo.create_workspace(database, WorkspaceCreate(name="W"))
    document_id = insert_document(database, workspace.id, "a.pdf", status="failed")
    database.execute(
        "UPDATE documents SET error = 'hỏng', indexed_at = '2020-01-01' WHERE id = ?",
        (document_id,),
    )

    await repo.queue_document(database, document_id, use_ocr=True)

    row = database.fetch_one(
        "SELECT status, error, indexed_at, use_ocr FROM documents WHERE id = ?",
        (document_id,),
    )
    assert row == {"status": "queued", "error": None, "indexed_at": None, "use_ocr": 1}


async def test_profiles_switch_and_the_last_one_cannot_be_deleted(
    database: Database,
) -> None:
    original = await repo.active_profile_id_async(database)
    created = await repo.create_profile(database, "Người thứ hai")
    # Creating a profile switches to it, which is the only reason to create one.
    assert created.active
    assert await repo.active_profile_id_async(database) == created.id

    await repo.activate_profile(database, original)
    await repo.delete_profile(database, created.id, confirmed=True)

    with pytest.raises(ValueError, match="last profile"):
        await repo.delete_profile(database, original, confirmed=True)


async def test_deleting_the_active_profile_moves_the_pointer(database: Database) -> None:
    created = await repo.create_profile(database, "Tạm")
    assert await repo.active_profile_id_async(database) == created.id

    await repo.delete_profile(database, created.id, confirmed=True)

    remaining = await repo.active_profile_id_async(database)
    assert remaining != created.id
    assert remaining


async def test_active_profile_falls_back_when_the_pointer_is_stale(
    database: Database,
) -> None:
    """A dangling pointer must not leave the app without an identity."""
    database.execute("UPDATE app_state SET value = 'ghost' WHERE key = 'active_profile_id'")
    assert repo.active_profile_id(database) == "local-user"
    assert await repo.active_profile_id_async(database) == "local-user"


async def test_memories_are_scoped_to_a_profile_and_hide_disabled_rows(
    database: Database,
) -> None:
    owner = await repo.active_profile_id_async(database)
    database.execute_many(
        """
        INSERT INTO memories(
            id, user_id, type, content, source, confidence, enabled, created_at, updated_at
        ) VALUES (?, ?, 'fact', ?, 'user', 0.9, ?, '2026-01-01', '2026-01-01')
        """,
        [
            ("m1", owner, "đang bật", 1),
            ("m2", owner, "đã tắt", 0),
            ("m3", "someone-else", "của người khác", 1),
        ],
    )

    visible = await repo.list_memories(database)
    assert [memory.content for memory in visible] == ["đang bật"]

    everything = await repo.list_memories(database, include_disabled=True)
    assert {memory.content for memory in everything} == {"đang bật", "đã tắt"}


async def test_model_defaults_reject_an_unknown_task(database: Database) -> None:
    await repo.set_model_default(database, "chat", "qwen3")
    assert (await repo.get_model_defaults(database))["chat"] == "qwen3"
    with pytest.raises(ValueError):
        await repo.set_model_default(database, "translation", "x")


async def test_model_events_are_newest_first_and_validated(database: Database) -> None:
    await repo.record_model_event(database, "qwen3", "pull", "completed")
    await repo.record_model_event(database, "qwen3", "delete", "failed", "bận")
    events = await repo.list_model_events(database)
    assert [event["action"] for event in events][:2] == ["delete", "pull"]
    with pytest.raises(ValueError):
        await repo.record_model_event(database, "qwen3", "pull", "maybe")


async def test_mcp_server_rows_decode_their_json_columns(database: Database) -> None:
    database.execute(
        """
        INSERT INTO mcp_servers(
            id, name, kind, command, args_json, url, headers_json, enabled,
            created_at, updated_at
        ) VALUES ('1', 'ngoai', 'stdio', 'uvx', '["a","b"]', '', '{"X":"1"}', 1,
                  '2026-01-01', '2026-01-01')
        """
    )
    database.execute(
        """
        INSERT INTO mcp_servers(
            id, name, kind, command, args_json, url, headers_json, enabled,
            created_at, updated_at
        ) VALUES ('2', 'hong', 'http', '', 'không-phải-json', 'http://x', '{', 1,
                  '2026-01-01', '2026-01-01')
        """
    )

    servers = {server.name: server for server in await repo.list_mcp_servers(database)}
    assert servers["ngoai"].args == ["a", "b"]
    assert servers["ngoai"].headers == {"X": "1"}
    # Malformed JSON degrades to empty rather than taking the settings screen down.
    assert servers["hong"].args == []
    assert servers["hong"].headers == {}
