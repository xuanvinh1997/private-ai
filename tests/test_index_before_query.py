"""A document is queryable only after it has been indexed.

``status = 'ready'`` used to be set the moment a text file was copied in, before a single
chunk or vector existed. Retrieval keyed off that status, so a freshly uploaded file was
visible to search with an empty index — and the summary strategy, finding no chunks, fell
back to re-chunking the whole raw file into the prompt.

The contract now is: ``indexed_at`` is the proof, ``ready`` follows it, and everything in
between sits at ``STATUS_EXTRACTED`` where no query can reach it.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from conftest import insert_document

from private_ai.agent.graph import _indexing_count, _summary_plan
from private_ai.core.database import Database
from private_ai.core.services import AppServices
from private_ai.rag.ingestion.pipeline import STATUS_EXTRACTED

BODY = "# Báo cáo\n" + ("nội dung tài liệu quan trọng. " * 200)


async def _upload(services: AppServices, workspace_id: str, tmp_path: Path, name: str) -> str:
    source = tmp_path / name
    source.write_text(BODY, encoding="utf-8")
    return await services.ingestion.add_file(source, workspace_id)


@pytest.mark.asyncio
async def test_an_uploaded_text_file_is_not_ready_before_it_is_indexed(
    services: AppServices,
    database: Database,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    document_id = await _upload(services, workspace_id, tmp_path, "bao-cao.md")

    row = database.fetch_one(
        "SELECT status, indexed_at FROM documents WHERE id = ?", (document_id,)
    )
    assert row["status"] == STATUS_EXTRACTED
    assert row["indexed_at"] is None


@pytest.mark.asyncio
async def test_an_unindexed_document_is_invisible_to_retrieval(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    await _upload(services, workspace_id, tmp_path, "bao-cao.md")

    for strategy in ("vector", "keyword", "hybrid"):
        found = await services.strategies.retrieve(
            "nội dung tài liệu quan trọng",
            workspace_id=workspace_id,
            strategy=strategy,
        )
        assert found == [], f"{strategy} answered from a document that was never indexed"


@pytest.mark.asyncio
async def test_indexing_promotes_the_document_and_makes_it_queryable(
    services: AppServices,
    database: Database,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    document_id = await _upload(services, workspace_id, tmp_path, "bao-cao.md")
    await services.ingestion.process(document_id)

    row = database.fetch_one(
        "SELECT status, indexed_at FROM documents WHERE id = ?", (document_id,)
    )
    assert row["status"] == "ready"
    assert row["indexed_at"] is not None

    found = await services.strategies.retrieve(
        "nội dung tài liệu quan trọng",
        workspace_id=workspace_id,
        strategy="keyword",
    )
    assert found, "an indexed document must be reachable"
    assert found[0].metadata["document_id"] == document_id


@pytest.mark.asyncio
async def test_summary_will_not_scope_onto_an_unindexed_document(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    """The flood path: no chunks used to mean 'read the whole file instead'."""
    await _upload(services, workspace_id, tmp_path, "bao-cao.md")

    plan = await _summary_plan(
        services, "Tóm tắt toàn bộ tài liệu bao-cao.md", workspace_id, "auto"
    )
    assert plan is None


@pytest.mark.asyncio
async def test_the_turn_says_when_documents_are_still_indexing(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    assert await _indexing_count(services, workspace_id) == 0

    document_id = await _upload(services, workspace_id, tmp_path, "bao-cao.md")
    assert await _indexing_count(services, workspace_id) == 1

    await services.ingestion.process(document_id)
    assert await _indexing_count(services, workspace_id) == 0


@pytest.mark.asyncio
async def test_a_ready_row_without_indexed_at_is_still_treated_as_unindexed(
    services: AppServices,
    database: Database,
    workspace_id: str,
) -> None:
    """Legacy rows written by the old code must not answer out of a missing index."""
    insert_document(database, workspace_id, "cu.md", text=BODY, indexed=False)

    found = await services.strategies.retrieve(
        "nội dung tài liệu quan trọng",
        workspace_id=workspace_id,
        strategy="keyword",
    )
    assert found == []
    assert await _indexing_count(services, workspace_id) == 1
