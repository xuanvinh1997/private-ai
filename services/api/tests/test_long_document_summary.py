from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient

from private_ai_api.database import Database
from private_ai_api.schemas import ChatRequest
from private_ai_api.services import long_document_summary as summary
from private_ai_api.services.provider import ProviderUnavailable


def _insert_document(
    database: Database,
    *,
    document_id: str = "harry",
    filename: str = "harrypotter.pdf",
    workspace_id: str = "research",
) -> None:
    now = datetime.now(UTC).isoformat()
    database.execute(
        """
        INSERT INTO documents(
            id, workspace_id, filename, media_type, sha256, byte_size, status,
            source_path, extracted_text, index_mode, created_at, updated_at
        ) VALUES (?, ?, ?, 'application/pdf', ?, 100, 'ready', ?, '', 'simple', ?, ?)
        """,
        (document_id, workspace_id, filename, f"sha-{document_id}", filename, now, now),
    )
    chunks = (
        (0, "Bìa và thông tin xuất bản"),
        (10, "CONTENTS ONE The Beginning TWO The Middle THREE The End"),
        (11, "BOOK_ONE_EVENT_A Harry starts the first adventure."),
        (12, "BOOK_ONE_EVENT_B Harry completes the first adventure."),
        (20, "CONTENTS ONE A New Start TWO Another Trial THREE Home Again"),
        (21, "BOOK_TWO_EVENT This belongs to the next book."),
    )
    database.execute_many(
        """
        INSERT INTO document_chunks(
            id, document_id, chunk_index, content, section_level, created_at
        ) VALUES (?, ?, ?, ?, 0, ?)
        """,
        ((f"{document_id}-{index}", document_id, index, content, now) for index, content in chunks),
    )


@pytest.fixture
def database(tmp_path: Path) -> Database:
    value = Database(tmp_path / "summary.db")
    value.initialize()
    return value


def test_plan_uses_keyset_pages_and_stops_at_the_next_volume(
    database: Database,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _insert_document(database)
    monkeypatch.setattr(summary, "CHUNK_PAGE_SIZE", 2)

    plan = summary.build_summary_plan(
        database,
        "research",
        "Tóm tắt phần 1 truyện Harry Potter",
    )

    assert plan is not None
    assert plan.filename == "harrypotter.pdf"
    assert plan.volume == 1
    assert [chunk.index for chunk in plan.chunks] == [10, 11, 12]
    assert "BOOK_TWO_EVENT" not in " ".join(chunk.content for chunk in plan.chunks)


def test_plan_does_not_replace_normal_top_k_rag(database: Database) -> None:
    _insert_document(database)

    assert summary.build_summary_plan(database, "research", "Harry đã gặp ai?") is None


def test_plan_keeps_paging_instead_of_rejecting_a_large_document(database: Database) -> None:
    now = datetime.now(UTC).isoformat()
    database.execute(
        """
        INSERT INTO documents(
            id, workspace_id, filename, media_type, sha256, byte_size, status,
            source_path, extracted_text, index_mode, created_at, updated_at
        ) VALUES (
            'giant', 'research', 'giant-book.txt', 'text/plain', 'sha-giant', 100,
            'ready', 'giant-book.txt', '', 'simple', ?, ?
        )
        """,
        (now, now),
    )
    database.execute_many(
        """
        INSERT INTO document_chunks(
            id, document_id, chunk_index, content, section_level, created_at
        ) VALUES (?, 'giant', ?, ?, 0, ?)
        """,
        ((f"giant-{index}", index, f"Nội dung {index}", now) for index in range(1_205)),
    )

    plan = summary.build_summary_plan(
        database,
        "research",
        "Tóm tắt toàn bộ tài liệu giant book",
    )

    assert plan is not None
    assert len(plan.chunks) == 1_205
    assert plan.chunks[0].index == 0
    assert plan.chunks[-1].index == 1_204


class SummaryAI:
    def __init__(self) -> None:
        self.requests: list[ChatRequest] = []

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        self.requests.append(request)
        prompt = request.messages[-1].content
        if "[Đoạn" in prompt:
            events = [
                marker
                for marker in ("BOOK_ONE_EVENT_A", "BOOK_ONE_EVENT_B", "BOOK_TWO_EVENT")
                if marker in prompt
            ]
            return {"message": {"content": "map:" + ",".join(events)}}
        return {"message": {"content": "Tóm tắt hoàn chỉnh [harrypotter.pdf]"}}


@pytest.mark.asyncio
async def test_map_reduce_reads_every_scoped_batch_in_source_order(
    database: Database,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _insert_document(database)
    monkeypatch.setattr(summary, "SOURCE_BATCH_CHARS", 80)
    plan = summary.build_summary_plan(
        database,
        "research",
        "Tóm tắt phần 1 truyện Harry Potter",
    )
    assert plan is not None
    ai = SummaryAI()
    events = [event async for event in summary.summarize_steps(plan, ai, "test-model")]

    source_prompts = [
        request.messages[-1].content
        for request in ai.requests
        if "[Đoạn" in request.messages[-1].content
    ]
    combined = "\n".join(source_prompts)
    assert combined.index("BOOK_ONE_EVENT_A") < combined.index("BOOK_ONE_EVENT_B")
    assert "BOOK_TWO_EVENT" not in combined
    assert events[-1] == {"type": "result", "answer": "Tóm tắt hoàn chỉnh [harrypotter.pdf]"}


class FlakySummaryAI(SummaryAI):
    def __init__(self, failures: int, error: str = "connection reset") -> None:
        super().__init__()
        self.failures = failures
        self.error = error

    async def chat(self, request: ChatRequest) -> dict[str, Any]:
        if self.failures > 0:
            self.requests.append(request)
            self.failures -= 1
            raise ProviderUnavailable(self.error)
        return await super().chat(request)


@pytest.mark.asyncio
async def test_map_step_retries_a_temporarily_unavailable_provider(
    database: Database,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _insert_document(database)
    monkeypatch.setattr(summary, "PROVIDER_RETRY_DELAYS", (0.0, 0.0, 0.0))
    plan = summary.build_summary_plan(
        database,
        "research",
        "Tóm tắt phần 1 truyện Harry Potter",
    )
    assert plan is not None
    ai = FlakySummaryAI(failures=2)

    events = [event async for event in summary.summarize_steps(plan, ai, "test-model")]

    retries = [event for event in events if event["type"] == "retry"]
    assert [event["attempt"] for event in retries] == [1, 2]
    assert events[-1]["type"] == "result"
    assert len(ai.requests) == 4


@pytest.mark.asyncio
async def test_map_step_does_not_retry_a_permanent_provider_rejection(
    database: Database,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _insert_document(database)
    monkeypatch.setattr(summary, "PROVIDER_RETRY_DELAYS", (0.0, 0.0, 0.0))
    plan = summary.build_summary_plan(
        database,
        "research",
        "Tóm tắt phần 1 truyện Harry Potter",
    )
    assert plan is not None
    ai = FlakySummaryAI(failures=5, error="HTTP 401 from provider")

    with pytest.raises(ProviderUnavailable, match="HTTP 401"):
        _ = [event async for event in summary.summarize_steps(plan, ai, "test-model")]

    assert len(ai.requests) == 1


def test_streaming_chat_routes_long_summary_away_from_top_k_rag(client: TestClient) -> None:
    _insert_document(client.app.state.services.database)
    ai = SummaryAI()
    client.app.state.services.ai = ai
    conversation = client.post(
        "/api/v1/workspaces/research/conversations",
        json={"model": "test-model"},
    ).json()

    with client.stream(
        "POST",
        f"/api/v1/conversations/{conversation['id']}/chat/stream",
        json={"model": "test-model", "content": "Tóm tắt phần 1 truyện Harry Potter"},
    ) as response:
        body = "\n".join(response.iter_lines())

    assert response.status_code == 200
    assert '"type":"tool"' in body
    assert '"type":"delta","content":"Tóm tắt hoàn chỉnh [harrypotter.pdf]"' in body
    assert '"type":"done"' in body
    assert any("BOOK_ONE_EVENT_A" in request.messages[-1].content for request in ai.requests)
    assert not any("BOOK_TWO_EVENT" in request.messages[-1].content for request in ai.requests)
