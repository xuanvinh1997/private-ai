"""Chunking, the cross-process ingestion claim, and the integrity guard.

The three things the pipeline's own module docstring calls out as "not refactorable
detail" are exactly the three things tested here.
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest
from langchain_core.documents import Document

from private_ai.config import Settings
from private_ai.core.services import AppServices
from private_ai.rag.ingestion.pipeline import CLAIM_STALE_SECONDS, IngestionPipeline
from private_ai.rag.ingestion.splitters import (
    DEFAULT_SECTION_TITLE,
    SectionAwareTextSplitter,
)
from private_ai.rag.stores.sqlite_vectorstore import SqliteVectorStore

# --- the splitter ---------------------------------------------------------


def test_a_heading_opens_a_section_and_every_chunk_remembers_it() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=200, chunk_overlap=20)
    chunks = splitter.split_marked_text("# Chương một\nnội dung một\n\n## Mục 1.1\nnội dung hai\n")

    titles = [
        (chunk.metadata["section_title"], chunk.metadata["section_level"]) for chunk in chunks
    ]
    assert titles == [("Chương một", 1), ("Mục 1.1", 2)]
    # The first heading opens section 0 rather than leaving an empty section in front.
    assert [chunk.metadata["section_index"] for chunk in chunks] == [0, 1]
    assert [chunk.metadata["chunk_index"] for chunk in chunks] == [0, 1]


def test_text_before_any_heading_lands_in_the_default_section() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=200, chunk_overlap=20)
    chunks = splitter.split_marked_text("mở đầu không có tiêu đề\n\n# Chương\nnội dung")
    assert chunks[0].metadata["section_title"] == DEFAULT_SECTION_TITLE
    assert chunks[1].metadata["section_title"] == "Chương"
    assert chunks[0].metadata["section_index"] == 0
    assert chunks[1].metadata["section_index"] == 1


def test_a_page_marker_sets_the_page_for_everything_after_it() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=200, chunk_overlap=20)
    chunks = splitter.split_marked_text(
        "<!-- private-ai-page:1 -->\ntrang một\n<!-- private-ai-page:2 -->\ntrang hai\n"
    )

    assert [chunk.metadata["page_number"] for chunk in chunks] == [1, 2]
    # The marker itself is a flush point, so it never appears in a chunk's text.
    assert all("private-ai-page" not in chunk.page_content for chunk in chunks)


def test_a_chunk_never_straddles_a_heading_or_a_page_break() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=10_000, chunk_overlap=0)
    chunks = splitter.split_marked_text("# A\nmột\n# B\nhai\n<!-- private-ai-page:9 -->\nba\n")
    # One huge chunk would have been legal by size alone; three flush points forbid it.
    assert len(chunks) == 3
    assert chunks[2].metadata["page_number"] == 9


def test_a_long_section_is_windowed_with_the_configured_overlap() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=100, chunk_overlap=30)
    body = " ".join(f"tu{index}" for index in range(200))

    chunks = splitter.split_text(body)

    assert len(chunks) > 1
    assert all(len(chunk) <= 100 for chunk in chunks)
    # Consecutive windows share text: that is what keeps a sentence cut in half findable.
    tail = chunks[0][-25:]
    assert tail in chunks[1] or chunks[1].startswith(chunks[0][-30:].lstrip())


def test_zero_overlap_still_covers_the_whole_text() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=50, chunk_overlap=0)
    body = "".join(f"{index:04d}" for index in range(60))
    chunks = splitter.split_text(body)
    assert "".join(chunks) == body


def test_the_splitter_reads_its_sizes_from_settings(settings: Settings) -> None:
    """1400 was picked against the embedding model in use; shrinking it silently
    would change what every stored vector means."""
    splitter = SectionAwareTextSplitter.from_settings(settings)
    assert splitter._chunk_size == settings.retrieval_chunk_size
    assert splitter._chunk_overlap == settings.retrieval_chunk_overlap


def test_empty_and_whitespace_input_produce_no_chunks() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=100, chunk_overlap=10)
    assert splitter.split_marked_text("") == []
    assert splitter.split_marked_text("   \n\n \t ") == []


def test_split_documents_numbers_chunks_across_the_whole_batch() -> None:
    splitter = SectionAwareTextSplitter(chunk_size=200, chunk_overlap=0)
    chunks = splitter.split_documents(
        [
            Document(page_content="# A\nmột", metadata={"document_id": "d"}),
            Document(page_content="# B\nhai", metadata={"document_id": "d"}),
        ]
    )
    assert [chunk.metadata["chunk_index"] for chunk in chunks] == [0, 1]
    assert all(chunk.metadata["document_id"] == "d" for chunk in chunks)


# --- the cross-process claim ---------------------------------------------


def _second_process(services: AppServices) -> IngestionPipeline:
    """A pipeline with a different owner string — what a second process looks like."""
    other = IngestionPipeline(
        services.database,
        services.vectors,
        services.graph,
        services.models,
        services.settings,
    )
    other._owner = "another-host:99999"
    return other


def test_only_one_process_can_hold_a_documents_claim(services: AppServices) -> None:
    """Two runs at once would delete and re-embed the chunks the first is still writing."""
    first = services.ingestion
    second = _second_process(services)

    assert first._acquire_claim("doc-1") is True
    assert second._acquire_claim("doc-1") is False
    # A different document is unaffected.
    assert second._acquire_claim("doc-2") is True


def test_the_owner_may_retake_its_own_claim(services: AppServices) -> None:
    pipeline = services.ingestion
    assert pipeline._acquire_claim("doc-1") is True
    assert pipeline._acquire_claim("doc-1") is True


def test_a_claim_whose_heartbeat_stopped_may_be_taken_over(
    services: AppServices,
) -> None:
    """A quiet claim belonged to a process that was killed; nothing else can free it."""
    first = services.ingestion
    second = _second_process(services)
    assert first._acquire_claim("doc-1") is True

    stale = (datetime.now(UTC) - timedelta(seconds=CLAIM_STALE_SECONDS + 5)).isoformat()
    services.database.execute(
        "UPDATE document_claims SET renewed_at = ? WHERE document_id = 'doc-1'",
        (stale,),
    )

    assert second._acquire_claim("doc-1") is True
    owner = services.database.fetch_one(
        "SELECT owner FROM document_claims WHERE document_id = 'doc-1'"
    )
    assert owner == {"owner": "another-host:99999"}


def test_renewing_keeps_a_claim_alive_and_only_the_owner_may_release_it(
    services: AppServices,
) -> None:
    first = services.ingestion
    second = _second_process(services)
    first._acquire_claim("doc-1")
    stale = (datetime.now(UTC) - timedelta(seconds=CLAIM_STALE_SECONDS + 5)).isoformat()
    services.database.execute(
        "UPDATE document_claims SET renewed_at = ? WHERE document_id = 'doc-1'",
        (stale,),
    )

    first._renew_claim("doc-1")
    assert second._acquire_claim("doc-1") is False

    second._release_claim("doc-1")
    assert (
        services.database.fetch_one("SELECT owner FROM document_claims WHERE document_id = 'doc-1'")
        is not None
    )

    first._release_claim("doc-1")
    assert (
        services.database.fetch_one("SELECT owner FROM document_claims WHERE document_id = 'doc-1'")
        is None
    )


async def test_process_leaves_a_document_alone_while_another_process_owns_it(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    document_id = await _add_text_file(
        services, workspace_id, tmp_path, "ghi-chu.md", "# A\nnội dung"
    )
    _second_process(services)._acquire_claim(document_id)

    await services.ingestion.process(document_id)

    # Nothing was written: the other process is mid-run on this very document.
    assert (
        services.database.fetch_all(
            "SELECT id FROM document_chunks WHERE document_id = ?",
            (document_id,),
        )
        == []
    )


def test_orphaned_jobs_are_failed_but_live_claims_are_left_alone(
    services: AppServices,
) -> None:
    """A job cannot outlive its process; without this it pins the UI at whatever
    percentage it died on."""
    database = services.database
    now = datetime.now(UTC).isoformat()
    stale = (datetime.now(UTC) - timedelta(seconds=CLAIM_STALE_SECONDS + 5)).isoformat()
    database.execute_many(
        "INSERT INTO jobs(id, document_id, kind, status, progress, payload_json, "
        "created_at, updated_at) VALUES (?, ?, 'ingest', 'processing', 0.5, '{}', ?, ?)",
        [("job-dead", "doc-dead", now, now), ("job-live", "doc-live", now, now)],
    )
    database.execute(
        "INSERT INTO document_claims(document_id, owner, claimed_at, renewed_at) "
        "VALUES ('doc-dead', 'ghost:1', ?, ?)",
        (stale, stale),
    )
    database.execute(
        "INSERT INTO document_claims(document_id, owner, claimed_at, renewed_at) "
        "VALUES ('doc-live', 'alive:2', ?, ?)",
        (now, now),
    )

    services.ingestion._recover_orphaned_jobs()

    statuses = {
        str(row["id"]): str(row["status"])
        for row in database.fetch_all("SELECT id, status FROM jobs")
    }
    assert statuses == {"job-dead": "failed", "job-live": "processing"}
    assert (
        database.fetch_one("SELECT owner FROM document_claims WHERE document_id = 'doc-dead'")
        is None
    )


# --- the integrity guard --------------------------------------------------


async def _add_text_file(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
    name: str,
    body: str,
) -> str:
    source = tmp_path / name
    source.write_text(body, encoding="utf-8")
    return await services.ingestion.add_file(source, workspace_id)


async def test_a_text_file_is_read_chunked_and_embedded(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    stages: list[tuple[str, float]] = []
    document_id = await _add_text_file(
        services,
        workspace_id,
        tmp_path,
        "ghi-chu.md",
        "# Chương một\n" + ("nội dung tài liệu. " * 200),
    )

    await services.ingestion.process(
        document_id,
        on_progress=lambda stage, progress, detail: stages.append((stage, progress)),
    )

    row = services.database.fetch_one(
        "SELECT status, indexed_at, error FROM documents WHERE id = ?",
        (document_id,),
    )
    assert row["status"] == "ready"
    assert row["indexed_at"] is not None
    assert row["error"] is None

    chunks = services.database.fetch_all(
        "SELECT embedding_vector FROM document_chunks WHERE document_id = ?",
        (document_id,),
    )
    assert chunks
    assert all(chunk["embedding_vector"] is not None for chunk in chunks)
    # The progress ladder ends where the documents view expects it to.
    assert stages[-1] == ("completed", 1.0)
    assert {stage for stage, _ in stages} >= {"chunking", "embedding", "completed"}


async def test_a_half_embedded_document_can_never_read_as_ready(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """``indexed_at`` alone never proves an index exists: the table is asked as well.

    The run below succeeds by its own counter — every batch was handed to the store and
    came back — but the rows it left behind carry no vector. Only asking the table
    catches that, which is the whole point of the guard.
    """

    async def wrote_rows_but_no_vectors(
        self: SqliteVectorStore,
        documents: list[Document],
        **kwargs: object,
    ) -> list[str]:
        created_at = datetime.now(UTC).isoformat()
        rows = [
            (
                f"chunk-{document.metadata['chunk_index']}",
                str(document.metadata["document_id"]),
                int(document.metadata["chunk_index"]),
                document.page_content,
                created_at,
            )
            for document in documents
        ]
        self.database.execute_many(
            "INSERT INTO document_chunks(id, document_id, chunk_index, content, "
            "embedding_vector, embedding_model, created_at) VALUES (?, ?, ?, ?, NULL, NULL, ?)",
            rows,
        )
        return [row[0] for row in rows]

    monkeypatch.setattr(SqliteVectorStore, "aadd_documents", wrote_rows_but_no_vectors)

    document_id = await _add_text_file(
        services, workspace_id, tmp_path, "ghi-chu.md", "# A\nnội dung tài liệu"
    )
    await services.ingestion.process(document_id)

    row = services.database.fetch_one(
        "SELECT status, indexed_at, error FROM documents WHERE id = ?",
        (document_id,),
    )
    assert row["status"] == "failed"
    assert row["indexed_at"] is None
    assert "chỉ mục" in str(row["error"])


async def test_process_pending_picks_up_a_document_whose_vectors_went_missing(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    document_id = await _add_text_file(
        services, workspace_id, tmp_path, "ghi-chu.md", "# A\nnội dung tài liệu"
    )
    await services.ingestion.process(document_id)
    assert services.database.fetch_one(
        "SELECT indexed_at FROM documents WHERE id = ?", (document_id,)
    )["indexed_at"]

    services.database.execute(
        "UPDATE document_chunks SET embedding_vector = NULL, embedding_json = NULL "
        "WHERE document_id = ?",
        (document_id,),
    )

    await services.ingestion.process_pending()

    chunks = services.database.fetch_all(
        "SELECT embedding_vector FROM document_chunks WHERE document_id = ?",
        (document_id,),
    )
    assert all(chunk["embedding_vector"] is not None for chunk in chunks)


async def test_the_same_bytes_are_not_stored_twice_in_one_workspace(
    services: AppServices,
    workspace_id: str,
    other_workspace_id: str,
    tmp_path: Path,
) -> None:
    first = await _add_text_file(services, workspace_id, tmp_path, "a.md", "cùng nội dung")
    again = await _add_text_file(services, workspace_id, tmp_path, "a.md", "cùng nội dung")
    assert again == first

    # Another workspace is another library, so the same file is a separate document.
    elsewhere = await _add_text_file(
        services, other_workspace_id, tmp_path, "a.md", "cùng nội dung"
    )
    assert elsewhere != first


async def test_a_file_over_the_upload_limit_is_refused(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    services.settings.max_upload_bytes = 10
    source = tmp_path / "big.md"
    source.write_text("x" * 100, encoding="utf-8")
    with pytest.raises(ValueError, match="dung lượng"):
        await services.ingestion.add_file(source, workspace_id)
    # Nothing was left behind in the library folder.
    assert not any(services.settings.documents_dir.iterdir())


async def test_a_file_for_an_unknown_workspace_is_refused(
    services: AppServices,
    tmp_path: Path,
) -> None:
    source = tmp_path / "a.md"
    source.write_text("nội dung", encoding="utf-8")
    with pytest.raises(LookupError):
        await services.ingestion.add_file(source, "không-tồn-tại")


async def test_index_text_stores_and_indexes_in_one_call(
    services: AppServices,
    workspace_id: str,
) -> None:
    document_id = await services.ingestion.index_text(
        workspace_id,
        "ghi-nho.md",
        "# Ghi nhớ\nnội dung cần tìm lại sau",
    )
    row = services.database.fetch_one(
        "SELECT status, index_mode, indexed_at FROM documents WHERE id = ?",
        (document_id,),
    )
    assert row["status"] == "ready"
    assert row["index_mode"] == "simple"
    assert row["indexed_at"] is not None

    with pytest.raises(ValueError, match="không được để trống"):
        await services.ingestion.index_text(workspace_id, "rong.md", "   ")
    with pytest.raises(ValueError, match="rag_mode"):
        await services.ingestion.index_text(workspace_id, "a.md", "nội dung", rag_mode="magic")


async def test_deleting_a_document_needs_confirmation_and_removes_its_files(
    services: AppServices,
    workspace_id: str,
    tmp_path: Path,
) -> None:
    document_id = await _add_text_file(services, workspace_id, tmp_path, "a.md", "# A\nnội dung")
    await services.ingestion.process(document_id)
    folder = services.settings.documents_dir / document_id
    assert folder.is_dir()

    with pytest.raises(PermissionError):
        await services.ingestion.delete_document(document_id)

    await services.ingestion.delete_document(document_id, confirmed=True)

    assert not folder.exists()
    assert (
        services.database.fetch_one("SELECT id FROM documents WHERE id = ?", (document_id,)) is None
    )
    assert (
        services.database.fetch_all(
            "SELECT id FROM document_chunks WHERE document_id = ?", (document_id,)
        )
        == []
    )


def test_ocr_is_the_documents_own_choice_before_the_global_default(
    services: AppServices,
    workspace_id: str,
) -> None:
    from conftest import insert_document

    document_id = insert_document(services.database, workspace_id, "scan.pdf")
    assert services.ingestion.ocr_enabled(document_id) is True

    services.database.execute("INSERT INTO app_state(key, value) VALUES ('ocr_enabled', '0')")
    assert services.ingestion.ocr_enabled(document_id) is False

    services.database.execute("UPDATE documents SET use_ocr = 1 WHERE id = ?", (document_id,))
    assert services.ingestion.ocr_enabled(document_id) is True


def test_the_vision_model_pick_prefers_the_stored_default(services: AppServices) -> None:
    services.settings.vision_model = "từ-cấu-hình"
    assert services.ingestion.resolve_vision_model() == "từ-cấu-hình"

    services.database.execute(
        "INSERT INTO model_defaults(task, model_name, updated_at) "
        "VALUES ('vision', 'người-dùng-chọn', '2026-01-01')"
    )
    assert services.ingestion.resolve_vision_model() == "người-dùng-chọn"
