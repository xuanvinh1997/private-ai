from __future__ import annotations

import json

import pytest
from fastapi.testclient import TestClient

workspace = {"workspace_id": "personal"}


class VehicleEmbedder:
    """Two dimensions: "is about vehicles" and "is not"."""

    async def embed(self, _model: str, inputs: list[str]) -> list[list[float]]:
        terms = ("motor vehicle", "automobile", "xe hoi", "o to")
        return [
            [1.0, 0.0] if any(term in value.casefold() for term in terms) else [0.0, 1.0]
            for value in inputs
        ]


def _upload(client: TestClient, name: str, body: bytes) -> str:
    response = client.post(
        "/api/v1/documents",
        files={"file": (name, body, "text/plain")},
        data=workspace,
    )
    assert response.status_code == 201
    return str(response.json()["id"])


def test_embeddings_are_stored_packed_rather_than_as_json(client: TestClient) -> None:
    """JSON text cost a parse per chunk on every search; float32 costs none."""
    database = client.app.state.services.database
    document_id = _upload(client, "vectors.txt", b"mot hai ba bon nam sau bay")

    chunks = database.fetch_all(
        "SELECT embedding_json, embedding_vector, embedding_model FROM document_chunks "
        "WHERE document_id = ?",
        (document_id,),
    )

    assert chunks
    for chunk in chunks:
        assert chunk["embedding_json"] is None
        assert isinstance(chunk["embedding_vector"], bytes)
        # FakeIndex embeds into two dimensions, so four bytes each.
        assert len(chunk["embedding_vector"]) == 8
        assert chunk["embedding_model"] == "test-embedding"


@pytest.mark.asyncio
async def test_semantic_ranking_survives_the_move_to_packed_vectors(
    client: TestClient,
) -> None:
    processor = client.app.state.services.document_processor
    processor.ai = VehicleEmbedder()
    car = _upload(client, "car.txt", b"The automobile is parked outside")
    _upload(client, "soup.txt", b"Recipe for pumpkin broth and bread")
    await processor.index_document(car)

    results = await processor._search_simple("motor vehicle", "personal", 5)

    assert results
    assert results[0]["document_id"] == car
    assert results[0]["score"] > 0.9


@pytest.mark.asyncio
async def test_indexes_written_before_the_change_still_rank(client: TestClient) -> None:
    """Existing installs must not silently lose semantic search on upgrade."""
    processor = client.app.state.services.document_processor
    database = client.app.state.services.database
    processor.ai = VehicleEmbedder()
    car = _upload(client, "legacy.txt", b"The automobile is parked outside")
    await processor.index_document(car)
    # Put the document back the way the old code stored it.
    database.execute(
        "UPDATE document_chunks SET embedding_vector = NULL, embedding_json = ? "
        "WHERE document_id = ?",
        (json.dumps([1.0, 0.0]), car),
    )

    results = await processor._search_simple("motor vehicle", "personal", 5)

    assert [row["document_id"] for row in results] == [car]
    assert results[0]["score"] > 0.9


@pytest.mark.asyncio
async def test_a_vector_of_the_wrong_width_is_ignored_not_fatal(client: TestClient) -> None:
    """Switching embedding model leaves old rows behind; they must not crash a search."""
    processor = client.app.state.services.document_processor
    processor.ai = VehicleEmbedder()
    car = _upload(client, "mismatch.txt", b"The automobile is parked outside")
    await processor.index_document(car)
    client.app.state.services.database.execute(
        "UPDATE document_chunks SET embedding_vector = ? WHERE document_id = ?",
        (b"\x00\x00\x80?" * 5, car),
    )

    results = await processor._search_simple("automobile", "personal", 5)

    # No semantic opinion, but the keyword path still finds it.
    assert [row["document_id"] for row in results] == [car]
