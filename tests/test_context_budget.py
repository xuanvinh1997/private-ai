"""Retrieved context must never be able to fill the model's window.

The failure this guards against was real: ``SummaryStrategy.retrieve`` returns every chunk
of a document on purpose, so that a caller can map-reduce it, and the agent graph used to
hand that straight to ``document_block``. Asking for a summary of a large document put the
entire document in the system prompt.
"""

from __future__ import annotations

from uuid import uuid4

import pytest
from conftest import insert_document
from langchain_core.documents import Document

from private_ai.agent.prompts import (
    DEFAULT_CONTEXT_CHARS,
    build_system_prompt,
    document_block,
    summary_block,
)
from private_ai.core.database import Database
from private_ai.core.services import AppServices

CHUNK = "Điều khoản {n}. " + ("nội dung dài " * 60)


def _passages(count: int, size: int = 2000) -> list[Document]:
    return [
        Document(
            page_content=f"đoạn {index} " + "x" * size,
            metadata={"filename": "bao-cao.pdf", "document_id": "d1", "chunk_id": f"d1:{index}"},
        )
        for index in range(count)
    ]


def test_document_block_stays_inside_its_budget() -> None:
    block = document_block(_passages(200), budget=8000)
    # Header and truncation markers add a little, but the body cannot run away.
    assert len(block) < 8000 * 1.4


def test_document_block_shares_the_budget_across_passages() -> None:
    block = document_block(_passages(4), budget=8000)
    # Every passage is represented rather than the first one eating the whole allowance.
    for index in range(4):
        assert f"đoạn {index} " in block


def test_document_block_says_when_it_trimmed() -> None:
    block = document_block(_passages(50), budget=4000)
    assert "bị cắt bớt" in block or "bị lược bỏ" in block


def test_a_small_result_set_is_not_truncated() -> None:
    passages = _passages(3, size=200)
    block = document_block(passages, budget=DEFAULT_CONTEXT_CHARS)
    assert "bị cắt bớt" not in block
    for passage in passages:
        assert passage.page_content in block


def test_a_digest_replaces_the_passages_it_was_built_from() -> None:
    """The whole point: the prompt carries the reduction, not the source."""
    prompt = build_system_prompt(
        documents=_passages(300),
        summary="Báo cáo trình bày ba nội dung chính.",
        summary_label="bao-cao.pdf",
    )
    assert "Báo cáo trình bày ba nội dung chính." in prompt
    assert "đoạn 0 " not in prompt
    assert len(prompt) < DEFAULT_CONTEXT_CHARS


def test_summary_block_is_empty_for_empty_text() -> None:
    assert summary_block("   ", "x.pdf") == ""


@pytest.mark.asyncio
async def test_a_summary_request_over_a_large_document_does_not_flood_the_prompt(
    services: AppServices,
    database: Database,
    workspace_id: str,
) -> None:
    """End to end: a real document, a real summary request, a bounded system prompt."""
    from private_ai.agent.graph import _summary_plan

    document_id = insert_document(database, workspace_id, "bao-cao-lon.pdf", text="x")
    now_chunks = 400
    for index in range(now_chunks):
        database.execute(
            """
            INSERT INTO document_chunks(id, document_id, chunk_index, content, created_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            """,
            (str(uuid4()), document_id, index, CHUNK.format(n=index)),
        )

    query = "Tóm tắt toàn bộ tài liệu bao-cao-lon.pdf"
    plan = await _summary_plan(services, query, workspace_id, "auto")
    assert plan is not None, "an explicit summary request should produce a plan"
    assert len(plan.chunks) == now_chunks

    raw = sum(len(chunk.content) for chunk in plan.chunks)
    assert raw > 200_000, "fixture must be big enough that flooding would be obvious"

    # The old behaviour: every chunk straight into the prompt.
    flooded = document_block(services.strategies.get("summary").documents(plan), budget=10**9)
    assert len(flooded) > 200_000

    # The fixed behaviour: the same passages, under budget.
    bounded = document_block(
        services.strategies.get("summary").documents(plan),
        budget=services.settings.retrieval_context_chars,
    )
    assert len(bounded) < services.settings.retrieval_context_chars * 1.4
