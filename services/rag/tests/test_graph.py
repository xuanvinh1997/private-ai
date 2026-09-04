"""Graph store behaviour that SurrealQL, not Python, decides.

Every case here failed at least once while the module was written: `UPSERT` on an edge table
stores a row the arrow traversal cannot see, `RELATE` refuses a function call where it wants a
record, and the SDK returns only the first statement of a multi-statement query. Asserting on
those is the point - they catch a SurrealDB upgrade changing the semantics under us.
"""

from __future__ import annotations

import pytest

from pai_rag_service.config import GraphConfig
from pai_rag_service.graph import Edge, Entity, GraphStore, entity_key


@pytest.fixture
def store(tmp_path):
    graph = GraphStore(GraphConfig(), f"surrealkv://{tmp_path / 'graph'}", "test")
    yield graph
    graph.close()


def test_opens_and_answers(store):
    assert store.health()
    assert store.count() == (0, 0)


def test_names_normalise_to_one_entity():
    # NFC plus casefold, so the decomposed spelling and the capitalised one are the same record.
    assert entity_key("Hà Nội") == entity_key("hà nội")
    assert entity_key("Hà Nội") == entity_key("Hà Nội")


def test_writes_are_idempotent(store):
    for _ in range(2):
        store.upsert_entities(
            [Entity("Hà Nội", "place", (1, 2)), Entity("Việt Nam", "place", (2,))],
            document_id="doc-a",
        )
        store.relate([Edge("Hà Nội", "Việt Nam", "located_in")], document_id="doc-a")
    assert store.count() == (2, 1)


def test_neighbours_read_both_directions(store):
    store.upsert_entities([Entity("Hà Nội"), Entity("Việt Nam")], document_id="doc-a")
    store.relate([Edge("Hà Nội", "Việt Nam", "located_in")], document_id="doc-a")

    forward = store.neighbors("HÀ NỘI")
    assert [item.name for item in forward] == ["Việt Nam"]
    assert forward[0].via == "located_in"
    assert [item.name for item in store.neighbors("việt nam")] == ["Hà Nội"]


def test_chunks_bridge_back_to_retrieval(store):
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(1, 2))], document_id="doc-a")
    store.upsert_entities([Entity("Việt Nam", chunk_ids=(9,))], document_id="doc-b")
    assert sorted(store.chunks_for(["Hà Nội", "Việt Nam"])) == [1, 2, 9]
    assert store.chunks_for(["không có thực thể này"]) == []
    assert store.chunks_for([]) == []


def test_removing_a_document_leaves_the_others_intact(store):
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(1,))], document_id="doc-a")
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(7,))], document_id="doc-b")
    store.upsert_entities([Entity("Việt Nam", chunk_ids=(2,))], document_id="doc-a")
    store.relate([Edge("Hà Nội", "Việt Nam", "located_in")], document_id="doc-a")

    store.remove_document("doc-a")

    # `Hà Nội` is still named by doc-b, so it survives; `Việt Nam` was only in doc-a and goes,
    # and the relation goes with the document that stated it rather than outliving both its ends.
    assert store.chunks_for(["Hà Nội"]) == [7]
    assert store.count() == (1, 0)


def test_a_relation_two_documents_state_survives_losing_one(store):
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(1,))], document_id="doc-a")
    store.upsert_entities([Entity("Việt Nam", chunk_ids=(2,))], document_id="doc-b")
    store.relate([Edge("Hà Nội", "Việt Nam", "located_in")], document_id="doc-a")
    store.relate([Edge("Hà Nội", "Việt Nam", "located_in")], document_id="doc-b")

    store.remove_document("doc-a")
    assert store.count() == (2, 1)
    store.remove_document("doc-b")
    assert store.count() == (0, 0)


def test_drop_empties_the_graph_but_keeps_it_usable(store):
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(1,))], document_id="doc-a")
    store.drop()
    assert store.count() == (0, 0)
    store.upsert_entities([Entity("Hà Nội", chunk_ids=(1,))], document_id="doc-a")
    assert store.count() == (1, 0)
