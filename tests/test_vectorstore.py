"""The LangChain vector store over ``document_chunks``.

Three properties matter here and none of them are incidental: the packed float32 layout
(which is what made a workspace scan one numpy call), the workspace boundary, and the
rule that a row embedded by a different model is skipped rather than rescored.
"""

from __future__ import annotations

import json

import numpy as np
import pytest
from conftest import EMBEDDING_MODEL, insert_document
from langchain_core.documents import Document

from private_ai.core.services import AppServices
from private_ai.rag.stores.sqlite_vectorstore import (
    SqliteVectorStore,
    pack_vector,
    search_tokens,
    unpack_vector,
)


def test_pack_and_unpack_round_trip_through_float32() -> None:
    packed = pack_vector([1.0, -0.5, 0.25])
    # Four bytes per dimension, not the ~six a JSON digit string cost.
    assert isinstance(packed, bytes)
    assert len(packed) == 12
    assert np.allclose(unpack_vector(packed, None), [1.0, -0.5, 0.25])


def test_unpack_still_reads_the_legacy_json_column() -> None:
    """Installs that indexed before the change must not silently lose semantic search."""
    assert np.allclose(unpack_vector(None, json.dumps([0.1, 0.2])), [0.1, 0.2])
    assert unpack_vector(None, "không phải json") is None
    assert unpack_vector(None, None) is None
    # A truncated blob is not a vector of a different length; it is corruption.
    assert unpack_vector(b"\x00\x00\x00", None) is None
    assert unpack_vector(b"", None) is None


def test_search_tokens_keeps_vietnamese_words_whole() -> None:
    assert search_tokens("Nghị định 15/2024 về an toàn") == [
        "nghị",
        "định",
        "15",
        "2024",
        "về",
        "an",
        "toàn",
    ]
    # One-character tokens carry no signal and would match everything.
    assert search_tokens("a b cd") == ["cd"]


@pytest.fixture
async def stocked(services: AppServices, workspace_id: str) -> dict[str, str]:
    """One workspace with two documents whose vocabularies do not overlap."""
    database = services.database
    car = insert_document(database, workspace_id, "xe.txt")
    soup = insert_document(database, workspace_id, "sup.txt")
    store = services.vectors.scoped(workspace_id)
    await store.aadd_documents(
        [
            Document(
                page_content="chiếc xe hơi đậu ngoài sân",
                metadata={"document_id": car, "chunk_index": 0, "page": 3},
            ),
            Document(
                page_content="công thức nấu súp bí đỏ",
                metadata={"document_id": soup, "chunk_index": 0},
            ),
        ]
    )
    return {"workspace": workspace_id, "car": car, "soup": soup}


async def test_chunks_are_written_packed_with_their_model(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    rows = services.database.fetch_all(
        "SELECT embedding_json, embedding_vector, embedding_model, page_number "
        "FROM document_chunks ORDER BY document_id"
    )
    assert len(rows) == 2
    for row in rows:
        assert row["embedding_json"] is None
        assert isinstance(row["embedding_vector"], bytes)
        assert row["embedding_model"] == EMBEDDING_MODEL
    # ``page`` in the metadata lands in the ``page_number`` column the citation reads.
    assert {row["page_number"] for row in rows} == {3, None}


async def test_similarity_ranks_the_document_that_shares_words_first(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    store = services.vectors.scoped(stocked["workspace"])
    ranked = await store.asimilarity_search_with_score("chiếc xe hơi", k=5)

    assert [document.metadata["document_id"] for document, _ in ranked][0] == stocked["car"]
    assert ranked[0][1] > ranked[-1][1]
    # The metadata contract every citation is built from.
    top, score = ranked[0]
    assert top.metadata["filename"] == "xe.txt"
    assert top.metadata["chunk_id"]
    assert top.metadata["strategy"] == "vector"
    assert top.metadata["page"] == 3
    assert top.metadata["score"] == pytest.approx(round(score, 4))


async def test_keyword_search_scores_by_overlap_with_the_query(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    store = services.vectors.scoped(stocked["workspace"])
    ranked = await store.akeyword_search("súp bí đỏ", k=5)

    assert [document.metadata["document_id"] for document, _ in ranked] == [stocked["soup"]]
    assert ranked[0][1] == pytest.approx(1.0)
    assert ranked[0][0].metadata["strategy"] == "keyword"
    # A query with nothing but stop-length tokens finds nothing rather than everything.
    assert await store.akeyword_search("a", k=5) == []


async def test_another_workspace_never_sees_these_chunks(
    services: AppServices,
    stocked: dict[str, str],
    other_workspace_id: str,
) -> None:
    other = services.vectors.scoped(other_workspace_id)
    assert await other.asimilarity_search_with_score("chiếc xe hơi", k=5) == []
    assert await other.akeyword_search("chiếc xe hơi", k=5) == []
    assert await other.acount() == 0
    assert await services.vectors.acount(workspace_id=stocked["workspace"]) == 2


async def test_rows_embedded_by_another_model_are_skipped_not_rescored(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    """Their vectors live in a different space; comparing them produces confident noise."""
    services.database.execute(
        "UPDATE document_chunks SET embedding_model = 'other-model' WHERE document_id = ?",
        (stocked["car"],),
    )
    store = services.vectors.scoped(stocked["workspace"])

    ranked = await store.asimilarity_search_with_score("chiếc xe hơi", k=5, score_threshold=0.0)

    assert stocked["car"] not in {document.metadata["document_id"] for document, _ in ranked}
    # The lexical path has no such constraint, so the document is still findable.
    lexical = await store.akeyword_search("chiếc xe hơi", k=5)
    assert stocked["car"] in {document.metadata["document_id"] for document, _ in lexical}


async def test_a_vector_of_the_wrong_width_is_ignored_rather_than_fatal(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    services.database.execute(
        "UPDATE document_chunks SET embedding_vector = ? WHERE document_id = ?",
        (pack_vector([1.0] * 5), stocked["car"]),
    )
    store = services.vectors.scoped(stocked["workspace"])
    ranked = await store.asimilarity_search_with_score("chiếc xe hơi", k=5)
    assert stocked["car"] not in {document.metadata["document_id"] for document, _ in ranked}


async def test_only_ready_simple_documents_are_searched(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    """A queued document is half-written and a graph document is LightRAG's to answer."""
    store = services.vectors.scoped(stocked["workspace"])
    services.database.execute(
        "UPDATE documents SET status = 'processing' WHERE id = ?",
        (stocked["car"],),
    )
    assert await store.akeyword_search("chiếc xe hơi", k=5) == []

    services.database.execute(
        "UPDATE documents SET status = 'ready', index_mode = 'graph' WHERE id = ?",
        (stocked["car"],),
    )
    assert await store.akeyword_search("chiếc xe hơi", k=5) == []


async def test_deleting_a_document_drops_its_chunks(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    store = services.vectors.scoped(stocked["workspace"])
    await store.adelete_document(stocked["car"])
    assert await store.acount() == 1
    assert await store.adelete(document_id=stocked["soup"]) is True
    assert await store.acount() == 0
    assert await store.adelete(None) is None


async def test_chunk_indexes_continue_where_the_last_write_stopped(
    services: AppServices,
    workspace_id: str,
) -> None:
    document_id = insert_document(services.database, workspace_id, "a.txt")
    store = services.vectors.scoped(workspace_id)
    await store.aadd_texts(["một"], [{"document_id": document_id}])
    await store.aadd_texts(["hai"], [{"document_id": document_id}])

    indexes = [
        row["chunk_index"]
        for row in services.database.fetch_all(
            "SELECT chunk_index FROM document_chunks WHERE document_id = ? ORDER BY chunk_index",
            (document_id,),
        )
    ]
    assert indexes == [0, 1]


async def test_writing_without_a_document_id_or_a_model_is_refused(
    services: AppServices,
    workspace_id: str,
) -> None:
    store = services.vectors.scoped(workspace_id)
    with pytest.raises(ValueError, match="document_id"):
        await store.aadd_texts(["mồ côi"], [{}])
    with pytest.raises(ValueError, match="metadatas"):
        await store.aadd_texts(["a", "b"], [{"document_id": "x"}])

    unmodelled = SqliteVectorStore(
        services.database,
        services.vectors.embeddings,
        workspace_id=workspace_id,
        embedding_model="",
    )
    with pytest.raises(ValueError, match="mô hình nhúng"):
        await unmodelled.aadd_texts(["a"], [{"document_id": "x"}])


async def test_the_synchronous_api_refuses_to_run_inside_the_loop(
    services: AppServices,
    workspace_id: str,
) -> None:
    """The whole app shares one loop, so blocking on it from inside itself would deadlock."""
    store = services.vectors.scoped(workspace_id)
    with pytest.raises(RuntimeError, match="synchronous API"):
        store.similarity_search("bất kỳ")


async def test_an_empty_query_or_workspace_yields_nothing(
    services: AppServices,
    stocked: dict[str, str],
) -> None:
    store = services.vectors.scoped(stocked["workspace"])
    assert await store.asimilarity_search_with_score("   ", k=5) == []
    assert await services.vectors.asimilarity_search_with_score("xe", k=5) == []
