from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from private_ai_api.database import Database
from private_ai_api.services.graph_store import GraphStore


class RecordingDriver:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def execute_query(self, query: str, **parameters: Any) -> tuple[list[Any], None, None]:
        self.calls.append((" ".join(query.split()), parameters))
        return [], None, None


def test_delete_document_removes_chunks_relationships_and_orphan_entities() -> None:
    graph = GraphStore(
        database=None,  # type: ignore[arg-type]
        url="bolt://127.0.0.1:7687",
        user="neo4j",
        password="test-password",
    )
    driver = RecordingDriver()
    graph._driver = driver
    graph._initialized = True

    assert asyncio.run(graph.delete_document("document-1")) is True

    queries = [query for query, _ in driver.calls]
    assert any(
        "MATCH (c:Chunk {document_id: $id}) DETACH DELETE c" in query
        for query in queries
    )
    assert any(
        "MATCH (d:Document {id: $id}) DETACH DELETE d" in query
        for query in queries
    )
    assert any(
        "MATCH ()-[r:RELATED_TO {document_id: $id}]->() DELETE r" in query
        for query in queries
    )
    assert any(
        "WHERE NOT (e)<-[:MENTIONS]-(:Chunk) DETACH DELETE e" in query
        for query in queries
    )
    assert all(parameters.get("id") == "document-1" for _, parameters in driver.calls[:3])


def test_delete_memory_removes_graph_node_and_orphan_user() -> None:
    graph = GraphStore(
        database=None,  # type: ignore[arg-type]
        url="bolt://127.0.0.1:7687",
        user="neo4j",
        password="test-password",
    )
    driver = RecordingDriver()
    graph._driver = driver
    graph._initialized = True

    assert asyncio.run(graph.delete_memory("memory-1")) is True

    queries = [query for query, _ in driver.calls]
    assert any("MATCH (m:Memory {id: $id}) DETACH DELETE m" in query for query in queries)
    assert any("NOT (u)<-[:BELONGS_TO]-(:Memory) DETACH DELETE u" in query for query in queries)


def test_sync_document_creates_sections_and_typed_entity_labels(tmp_path: Path) -> None:
    database = Database(tmp_path / "private-ai.db")
    database.initialize()
    now = datetime.now(UTC).isoformat()
    database.execute(
        """
        INSERT INTO documents(
            id, filename, media_type, sha256, byte_size, status, source_path,
            extracted_text, error, created_at, updated_at
        ) VALUES ('doc-1', 'guide.md', 'text/markdown', 'sha', 10, 'ready',
                  'guide.md', '# Team', NULL, ?, ?)
        """,
        (now, now),
    )
    database.execute(
        """
        INSERT INTO document_sections(
            id, document_id, section_index, title, level, page_start, page_end, created_at
        ) VALUES ('section-1', 'doc-1', 0, 'Team', 1, 2, 2, ?)
        """,
        (now,),
    )
    database.execute(
        """
        INSERT INTO document_chunks(
            id, document_id, chunk_index, content, section_id, section_title,
            section_level, page_number, embedding_json, embedding_model,
            graph_model, created_at
        ) VALUES ('chunk-1', 'doc-1', 0, 'Alice works at OpenAI', 'section-1',
                  'Team', 1, 2, NULL, NULL, NULL, ?)
        """,
        (now,),
    )

    graph = GraphStore(
        database,
        url="bolt://127.0.0.1:7687",
        user="neo4j",
        password="test-password",
    )
    driver = RecordingDriver()
    graph._driver = driver
    graph._sync_document_sync("doc-1")
    queries = [query for query, _ in driver.calls]

    assert any("MERGE (s:Section {id: row.id})" in query for query in queries)
    assert any("MERGE (d)-[:HAS_SECTION]->(s)" in query for query in queries)
    assert any("MERGE (s)-[:HAS_CHUNK]->(c)" in query for query in queries)
    assert any("SET e:Person" in query and "SET e:Organization" in query for query in queries)


def test_graph_expansion_uses_up_to_two_relationship_hops() -> None:
    graph = GraphStore(
        database=None,  # type: ignore[arg-type]
        url="bolt://127.0.0.1:7687",
        user="neo4j",
        password="test-password",
    )
    driver = RecordingDriver()
    graph._driver = driver

    assert graph._expand_chunks_sync(["chunk-1"], limit=5) == []
    query, parameters = driver.calls[0]
    assert "MATCH (start)-[:RELATED_TO]-(related:Entity)" in query
    assert "MATCH (start)-[:RELATED_TO]-(:Entity)-[:RELATED_TO]-(related:Entity)" in query
    assert parameters["seed_ids"] == ["chunk-1"]
