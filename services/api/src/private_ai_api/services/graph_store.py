from __future__ import annotations

import asyncio
import json
import re
from collections.abc import Iterable
from typing import Any

from neo4j import GraphDatabase
from neo4j.exceptions import Neo4jError, ServiceUnavailable
from neo4j_graphrag.indexes import create_fulltext_index, create_vector_index
from neo4j_graphrag.retrievers import HybridRetriever
from neo4j_graphrag.types import RetrieverResultItem

from private_ai_api.database import Database

VECTOR_INDEX = "private_ai_chunk_embeddings"
FULLTEXT_INDEX = "private_ai_chunk_fulltext"
MEMORY_VECTOR_INDEX = "private_ai_memory_embeddings"


def _format_search_record(record: Any) -> RetrieverResultItem:
    node = record.get("node")
    content = dict(node) if node is not None else {}
    content.pop("embedding", None)
    return RetrieverResultItem(content=content, metadata={"score": record.get("score")})


class GraphStore:
    """Optional Neo4j projection of the canonical SQLite document store."""

    def __init__(
        self,
        database: Database,
        *,
        url: str,
        user: str,
        password: str,
        neo4j_database: str = "neo4j",
        enabled: bool = True,
    ) -> None:
        self.database = database
        self.url = url
        self.user = user
        self.password = password
        self.neo4j_database = neo4j_database
        self.enabled = enabled and bool(password)
        self._driver: Any | None = None
        self._initialized = False
        self._vector_dimensions: int | None = None
        self._lock = asyncio.Lock()

    def _get_driver(self) -> Any:
        if self._driver is None:
            self._driver = GraphDatabase.driver(self.url, auth=(self.user, self.password))
        return self._driver

    async def health(self) -> bool:
        if not self.enabled:
            return False
        try:
            await asyncio.to_thread(self._get_driver().verify_connectivity)
            return True
        except (Neo4jError, ServiceUnavailable, OSError):
            return False

    async def initialize(self) -> bool:
        if not self.enabled:
            return False
        async with self._lock:
            try:
                await asyncio.to_thread(self._initialize_sync)
                self._initialized = True
                return True
            except (Neo4jError, ServiceUnavailable, OSError, ValueError):
                self._initialized = False
                return False

    def _initialize_sync(self) -> None:
        driver = self._get_driver()
        driver.verify_connectivity()
        for query in (
            "CREATE CONSTRAINT private_ai_document_id IF NOT EXISTS "
            "FOR (n:Document) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT private_ai_chunk_id IF NOT EXISTS "
            "FOR (n:Chunk) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT private_ai_section_id IF NOT EXISTS "
            "FOR (n:Section) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT private_ai_entity_key IF NOT EXISTS "
            "FOR (n:Entity) REQUIRE n.key IS UNIQUE",
            "CREATE CONSTRAINT private_ai_memory_id IF NOT EXISTS "
            "FOR (n:Memory) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT private_ai_user_id IF NOT EXISTS "
            "FOR (n:User) REQUIRE n.id IS UNIQUE",
        ):
            driver.execute_query(query, database_=self.neo4j_database)
        create_fulltext_index(
            driver,
            FULLTEXT_INDEX,
            label="Chunk",
            node_properties=["content", "filename"],
            neo4j_database=self.neo4j_database,
        )
        dimension = self._embedding_dimension()
        if dimension:
            create_vector_index(
                driver,
                VECTOR_INDEX,
                label="Chunk",
                embedding_property="embedding",
                dimensions=dimension,
                similarity_fn="cosine",
                neo4j_database=self.neo4j_database,
            )
            create_vector_index(
                driver,
                MEMORY_VECTOR_INDEX,
                label="Memory",
                embedding_property="embedding",
                dimensions=dimension,
                similarity_fn="cosine",
                neo4j_database=self.neo4j_database,
            )
            self._vector_dimensions = dimension
        self._sync_all_sync()

    def _embedding_dimension(self) -> int | None:
        row = self.database.fetch_one(
            "SELECT embedding_json FROM document_chunks "
            "WHERE embedding_json IS NOT NULL LIMIT 1"
        )
        if not row:
            row = self.database.fetch_one(
                "SELECT embedding_json FROM memories "
                "WHERE embedding_json IS NOT NULL LIMIT 1"
            )
        if not row:
            return None
        vector = json.loads(str(row["embedding_json"]))
        return len(vector) if isinstance(vector, list) and vector else None

    async def sync_all(self) -> bool:
        if not self._initialized and not await self.initialize():
            return False
        try:
            await asyncio.to_thread(self._sync_all_sync)
            return True
        except (Neo4jError, ServiceUnavailable, OSError, ValueError):
            return False

    def _sync_all_sync(self) -> None:
        documents = self.database.fetch_all("SELECT id FROM documents WHERE status = 'ready'")
        ids = [str(item["id"]) for item in documents]
        self._get_driver().execute_query(
            "MATCH (c:Chunk) WHERE NOT c.document_id IN $ids DETACH DELETE c",
            ids=ids,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH (s:Section) WHERE NOT s.document_id IN $ids DETACH DELETE s",
            ids=ids,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH (d:Document) WHERE NOT d.id IN $ids DETACH DELETE d",
            ids=ids,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH ()-[r:RELATED_TO]->() WHERE NOT r.document_id IN $ids DELETE r",
            ids=ids,
            database_=self.neo4j_database,
        )
        self._delete_orphan_entities_sync()
        for document_id in ids:
            self._sync_document_sync(document_id)
        self._sync_memories_sync()
        self._delete_orphan_entities_sync()

    async def sync_document(self, document_id: str) -> bool:
        if not self._initialized and not await self.initialize():
            return False
        try:
            await asyncio.to_thread(self._ensure_vector_index_sync)
            await asyncio.to_thread(self._sync_document_sync, document_id)
            return True
        except (Neo4jError, ServiceUnavailable, OSError, ValueError):
            return False

    def _ensure_vector_index_sync(self) -> None:
        dimension = self._embedding_dimension()
        if not dimension or dimension == self._vector_dimensions:
            return
        create_vector_index(
            self._get_driver(),
            VECTOR_INDEX,
            label="Chunk",
            embedding_property="embedding",
            dimensions=dimension,
            similarity_fn="cosine",
            neo4j_database=self.neo4j_database,
        )
        create_vector_index(
            self._get_driver(),
            MEMORY_VECTOR_INDEX,
            label="Memory",
            embedding_property="embedding",
            dimensions=dimension,
            similarity_fn="cosine",
            neo4j_database=self.neo4j_database,
        )
        self._vector_dimensions = dimension

    def _sync_memories_sync(self) -> None:
        memories = self.database.fetch_all("SELECT id FROM memories")
        ids = [str(memory["id"]) for memory in memories]
        self._get_driver().execute_query(
            "MATCH (m:Memory) WHERE NOT m.id IN $ids DETACH DELETE m",
            ids=ids,
            database_=self.neo4j_database,
        )
        for memory_id in ids:
            self._sync_memory_sync(memory_id)
        self._get_driver().execute_query(
            "MATCH (u:User) WHERE NOT (u)<-[:BELONGS_TO]-(:Memory) DETACH DELETE u",
            database_=self.neo4j_database,
        )

    async def sync_memory(self, memory_id: str) -> bool:
        if not self._initialized and not await self.initialize():
            return False
        try:
            await asyncio.to_thread(self._ensure_vector_index_sync)
            await asyncio.to_thread(self._sync_memory_sync, memory_id)
            return True
        except (Neo4jError, ServiceUnavailable, OSError, ValueError):
            return False

    def _sync_memory_sync(self, memory_id: str) -> None:
        memory = self.database.fetch_one(
            "SELECT id, user_id, type, content, source, confidence, enabled, "
            "created_at, updated_at, expires_at, embedding_json, embedding_model "
            "FROM memories WHERE id = ?",
            (memory_id,),
        )
        if not memory:
            return
        embedding = (
            json.loads(str(memory["embedding_json"]))
            if memory["embedding_json"]
            else None
        )
        self._get_driver().execute_query(
            """
            MERGE (u:User {id: $memory.user_id})
            MERGE (m:Memory {id: $memory.id})
            SET m.user_id = $memory.user_id,
                m.type = $memory.type,
                m.content = $memory.content,
                m.source = $memory.source,
                m.confidence = $memory.confidence,
                m.enabled = $memory.enabled,
                m.created_at = $memory.created_at,
                m.updated_at = $memory.updated_at,
                m.expires_at = coalesce($memory.expires_at, ''),
                m.embedding = $embedding,
                m.embedding_model = $memory.embedding_model
            MERGE (m)-[:BELONGS_TO]->(u)
            """,
            memory={
                **memory,
                "enabled": bool(memory["enabled"]),
                "embedding_model": memory["embedding_model"],
            },
            embedding=embedding,
            database_=self.neo4j_database,
        )

    def _sync_document_sync(self, document_id: str) -> None:
        document = self.database.fetch_one(
            "SELECT id, filename, media_type, sha256, created_at, updated_at "
            "FROM documents WHERE id = ? AND status = 'ready'",
            (document_id,),
        )
        if not document:
            return
        chunks = self.database.fetch_all(
            "SELECT id, chunk_index, content, section_id, section_title, section_level, "
            "page_number, embedding_json, embedding_model, graph_model "
            "FROM document_chunks WHERE document_id = ? ORDER BY chunk_index",
            (document_id,),
        )
        sections = self.database.fetch_all(
            "SELECT id, section_index, title, level, page_start, page_end "
            "FROM document_sections WHERE document_id = ? ORDER BY section_index",
            (document_id,),
        )
        stored_entities = self.database.fetch_all(
            "SELECT chunk_id, key, name, kind FROM chunk_entities WHERE document_id = ?",
            (document_id,),
        )
        stored_relations = self.database.fetch_all(
            "SELECT chunk_id, source_key, target_key, relation "
            "FROM chunk_relations WHERE document_id = ?",
            (document_id,),
        )
        entities_by_chunk: dict[str, list[dict[str, str]]] = {}
        for entity in stored_entities:
            entities_by_chunk.setdefault(str(entity["chunk_id"]), []).append(
                {
                    "key": str(entity["key"]),
                    "name": str(entity["name"]),
                    "kind": str(entity["kind"]),
                }
            )
        payload: list[dict[str, Any]] = []
        for chunk in chunks:
            embedding = (
                json.loads(str(chunk["embedding_json"]))
                if chunk["embedding_json"]
                else None
            )
            payload.append(
                {
                    "id": chunk["id"],
                    "chunk_index": chunk["chunk_index"],
                    "content": chunk["content"],
                    "section_id": chunk["section_id"],
                    "section_title": chunk["section_title"],
                    "section_level": chunk["section_level"],
                    "page_number": chunk["page_number"],
                    "embedding": embedding,
                    "embedding_model": chunk["embedding_model"],
                    "entities": (
                        entities_by_chunk.get(str(chunk["id"]), [])
                        if chunk["graph_model"]
                        else self.extract_entities(str(chunk["content"]))
                    ),
                    "graph_extracted": bool(chunk["graph_model"]),
                }
            )
        relation_payload = [
            {
                "chunk_id": str(relation["chunk_id"]),
                "source_key": str(relation["source_key"]),
                "target_key": str(relation["target_key"]),
                "relation": str(relation["relation"]),
            }
            for relation in stored_relations
        ]
        driver = self._get_driver()
        driver.execute_query(
            "MATCH ()-[r:RELATED_TO {document_id: $document_id}]->() DELETE r",
            document_id=document_id,
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            MATCH (d:Document {id: $document_id})
            OPTIONAL MATCH (d)-[:HAS_SECTION]->(old:Section)
            WHERE NOT old.id IN $section_ids
            DETACH DELETE old
            """,
            document_id=document_id,
            section_ids=[str(section["id"]) for section in sections],
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            MATCH (d:Document {id: $document_id})
            UNWIND $sections AS row
            MERGE (s:Section {id: row.id})
            SET s.document_id = $document_id,
                s.section_index = row.section_index,
                s.title = row.title,
                s.level = row.level,
                s.page_start = row.page_start,
                s.page_end = row.page_end
            MERGE (d)-[:HAS_SECTION]->(s)
            """,
            document_id=document_id,
            sections=sections,
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            MERGE (d:Document {id: $document.id})
            SET d.filename = $document.filename,
                d.media_type = $document.media_type,
                d.sha256 = $document.sha256,
                d.created_at = $document.created_at,
                d.updated_at = $document.updated_at
            WITH d
            OPTIONAL MATCH (d)-[:HAS_CHUNK]->(old:Chunk)
            WHERE NOT old.id IN $chunk_ids
            DETACH DELETE old
            """,
            document=dict(document),
            chunk_ids=[str(chunk["id"]) for chunk in chunks],
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            MATCH (d:Document {id: $document_id})
            UNWIND $chunks AS row
            MERGE (c:Chunk {id: row.id})
            SET c.document_id = $document_id,
                c.filename = $filename,
                c.chunk_index = row.chunk_index,
                c.content = row.content,
                c.section_id = row.section_id,
                c.section_title = row.section_title,
                c.section_level = row.section_level,
                c.page_number = row.page_number,
                c.embedding = row.embedding,
                c.embedding_model = row.embedding_model,
                c.graph_extracted = row.graph_extracted
            MERGE (d)-[:HAS_CHUNK]->(c)
            WITH d, c, row
            MATCH (s:Section {id: row.section_id})
            MERGE (s)-[:HAS_CHUNK]->(c)
            WITH c, row
            OPTIONAL MATCH (c)-[old:MENTIONS]->(:Entity)
            DELETE old
            WITH c, row
            UNWIND row.entities AS entity
            MERGE (e:Entity {key: entity.key})
            REMOVE e:Person:Organization:Concept
            SET e.name = entity.name, e.kind = entity.kind
            FOREACH (_ IN CASE WHEN toLower(entity.kind) = 'person' THEN [1] ELSE [] END |
                SET e:Person)
            FOREACH (_ IN CASE WHEN toLower(entity.kind) = 'organization' THEN [1] ELSE [] END |
                SET e:Organization)
            FOREACH (_ IN CASE WHEN toLower(entity.kind) = 'concept' THEN [1] ELSE [] END |
                SET e:Concept)
            MERGE (c)-[:MENTIONS]->(e)
            """,
            document_id=document_id,
            filename=document["filename"],
            chunks=payload,
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            MATCH (d:Document {id: $document_id})-[:HAS_CHUNK]->(c:Chunk)
            MATCH (c)-[:MENTIONS]->(a:Entity)
            MATCH (c)-[:MENTIONS]->(b:Entity)
            WHERE c.graph_extracted = false AND a.key < b.key
            MERGE (a)-[r:RELATED_TO {
                document_id: $document_id,
                chunk_id: c.id,
                kind: 'co_occurs'
            }]->(b)
            SET r.kind = 'co_occurs'
            """,
            document_id=document_id,
            database_=self.neo4j_database,
        )
        driver.execute_query(
            """
            UNWIND $relations AS row
            MATCH (a:Entity {key: row.source_key})
            MATCH (b:Entity {key: row.target_key})
            MERGE (a)-[r:RELATED_TO {
                document_id: $document_id,
                chunk_id: row.chunk_id,
                kind: row.relation
            }]->(b)
            SET r.kind = row.relation, r.source = 'ollama'
            """,
            document_id=document_id,
            relations=relation_payload,
            database_=self.neo4j_database,
        )
        self._delete_orphan_entities_sync()

    @staticmethod
    def extract_entities(content: str, limit: int = 12) -> list[dict[str, str]]:
        candidates: list[tuple[str, str]] = []
        for heading in re.findall(r"(?m)^#{1,6}\s+([^\n]{2,100})", content):
            candidates.append((heading.strip(), "concept"))
        capitalized = r"(?:[A-Z][A-Za-z0-9.+-]{1,}|[A-Z]{2,})"
        pattern = rf"\b{capitalized}(?:\s+{capitalized}){{0,3}}\b"
        candidates.extend((match.strip(), "entity") for match in re.findall(pattern, content))
        seen: set[str] = set()
        result: list[dict[str, str]] = []
        for name, kind in candidates:
            key = re.sub(r"\s+", " ", name).casefold()
            if len(key) < 2 or key in seen:
                continue
            seen.add(key)
            result.append({"key": key, "name": name, "kind": kind})
            if len(result) >= limit:
                break
        return result

    async def search(
        self,
        query: str,
        query_vector: list[float],
        limit: int = 5,
    ) -> list[dict[str, object]]:
        if not query_vector or (not self._initialized and not await self.initialize()):
            return []
        try:
            return await asyncio.to_thread(self._search_sync, query, query_vector, limit)
        except (Neo4jError, ServiceUnavailable, OSError, ValueError):
            return []

    def _search_sync(
        self,
        query: str,
        query_vector: list[float],
        limit: int,
    ) -> list[dict[str, object]]:
        retriever = HybridRetriever(
            self._get_driver(),
            VECTOR_INDEX,
            FULLTEXT_INDEX,
            return_properties=[
                "id",
                "document_id",
                "filename",
                "chunk_index",
                "section_id",
                "section_title",
                "section_level",
                "page_number",
                "content",
            ],
            result_formatter=_format_search_record,
            neo4j_database=self.neo4j_database,
        )
        result = retriever.search(
            query_text=query,
            query_vector=query_vector,
            top_k=max(1, min(limit, 20)),
        )
        records: list[dict[str, object]] = []
        for item in result.items:
            content = item.content if isinstance(item.content, dict) else {}
            records.append(
                {
                    "chunk_id": content.get("id"),
                    "document_id": content.get("document_id"),
                    "filename": content.get("filename"),
                    "chunk_index": content.get("chunk_index"),
                    "section_id": content.get("section_id"),
                    "section_title": content.get("section_title"),
                    "section_level": content.get("section_level"),
                    "page_number": content.get("page_number"),
                    "content": content.get("content"),
                    "score": float((item.metadata or {}).get("score") or 0.0),
                    "retrieval_source": "hybrid",
                }
            )
        records = [record for record in records if record["chunk_id"]]
        expanded = self._expand_chunks_sync(
            [str(record["chunk_id"]) for record in records],
            limit=max(1, min(limit, 20)),
        )
        target = max(1, min(limit, 20))
        primary_count = max(1, target // 2)
        merged = records[:primary_count]
        seen = {str(record["chunk_id"]) for record in merged}
        for record in [*expanded, *records[primary_count:]]:
            chunk_id = str(record["chunk_id"])
            if chunk_id in seen:
                continue
            merged.append(record)
            seen.add(chunk_id)
            if len(merged) >= target:
                break
        return merged

    def _expand_chunks_sync(
        self,
        seed_ids: list[str],
        *,
        limit: int,
    ) -> list[dict[str, object]]:
        if not seed_ids:
            return []
        records, _, _ = self._get_driver().execute_query(
            """
            UNWIND $seed_ids AS seed_id
            MATCH (seed:Chunk {id: seed_id})-[:MENTIONS]->(start:Entity)
            WITH DISTINCT seed, start
            LIMIT $seed_entity_limit
            CALL (start) {
                MATCH (start)-[:RELATED_TO]-(related:Entity)
                RETURN DISTINCT related, 1 AS graph_hops
                LIMIT $per_hop_limit
                UNION
                MATCH (start)-[:RELATED_TO]-(:Entity)-[:RELATED_TO]-(related:Entity)
                WHERE related <> start
                RETURN DISTINCT related, 2 AS graph_hops
                LIMIT $per_hop_limit
            }
            MATCH (related)<-[:MENTIONS]-(chunk:Chunk)
            WHERE chunk.id <> seed.id
            RETURN chunk.id AS chunk_id,
                   chunk.document_id AS document_id,
                   chunk.filename AS filename,
                   chunk.chunk_index AS chunk_index,
                   chunk.section_id AS section_id,
                   chunk.section_title AS section_title,
                   chunk.section_level AS section_level,
                   chunk.page_number AS page_number,
                   chunk.content AS content,
                   min(graph_hops) AS graph_hops,
                   count(DISTINCT start) AS shared_entities
            ORDER BY graph_hops ASC, shared_entities DESC, chunk_index ASC
            LIMIT $limit
            """,
            seed_ids=seed_ids,
            seed_entity_limit=16,
            per_hop_limit=8,
            limit=max(1, min(limit, 20)),
            database_=self.neo4j_database,
        )
        return [
            {
                **dict(record),
                "score": round(1.0 / (2 + int(record["graph_hops"] or 0)), 4),
                "retrieval_source": "graph_expansion",
            }
            for record in records
        ]

    async def search_memories(
        self,
        query_vector: list[float],
        *,
        user_id: str = "local-user",
        limit: int = 5,
    ) -> list[dict[str, object]]:
        if not query_vector or (not self._initialized and not await self.initialize()):
            return []
        try:
            await asyncio.to_thread(self._ensure_vector_index_sync)
            return await asyncio.to_thread(
                self._search_memories_sync,
                query_vector,
                user_id,
                limit,
            )
        except (Neo4jError, ServiceUnavailable, OSError, ValueError):
            return []

    def _search_memories_sync(
        self,
        query_vector: list[float],
        user_id: str,
        limit: int,
    ) -> list[dict[str, object]]:
        records, _, _ = self._get_driver().execute_query(
            """
            CALL db.index.vector.queryNodes($index_name, $candidate_limit, $query_vector)
            YIELD node, score
            WHERE node.user_id = $user_id
              AND node.enabled = true
              AND (node.expires_at = '' OR datetime(node.expires_at) > datetime())
            RETURN node.id AS id,
                   node.user_id AS user_id,
                   node.type AS type,
                   node.content AS content,
                   node.source AS source,
                   node.confidence AS confidence,
                   node.enabled AS enabled,
                   node.created_at AS created_at,
                   node.updated_at AS updated_at,
                   node.expires_at AS expires_at,
                   score
            ORDER BY score DESC
            LIMIT $limit
            """,
            index_name=MEMORY_VECTOR_INDEX,
            candidate_limit=max(20, min(limit * 5, 100)),
            query_vector=query_vector,
            user_id=user_id,
            limit=max(1, min(limit, 20)),
            database_=self.neo4j_database,
        )
        return [dict(record) for record in records]

    async def find_entities(self, query: str, limit: int = 20) -> list[dict[str, object]]:
        if not self._initialized and not await self.initialize():
            return []
        return await asyncio.to_thread(self._find_entities_sync, query, limit)

    def _find_entities_sync(self, query: str, limit: int) -> list[dict[str, object]]:
        records, _, _ = self._get_driver().execute_query(
            "MATCH (e:Entity) WHERE toLower(e.name) CONTAINS toLower($query) "
            "OPTIONAL MATCH (e)<-[:MENTIONS]-(c:Chunk) "
            "RETURN e.key AS key, e.name AS name, e.kind AS kind, count(c) AS mentions "
            "ORDER BY mentions DESC, name LIMIT $limit",
            query=query,
            limit=max(1, min(limit, 100)),
            database_=self.neo4j_database,
        )
        return [dict(record) for record in records]

    async def neighborhood(self, entity_key: str, limit: int = 30) -> dict[str, object]:
        if not self._initialized and not await self.initialize():
            return {"entity": None, "neighbors": [], "chunks": []}
        return await asyncio.to_thread(self._neighborhood_sync, entity_key, limit)

    def _neighborhood_sync(self, entity_key: str, limit: int) -> dict[str, object]:
        records, _, _ = self._get_driver().execute_query(
            """
            MATCH (e:Entity {key: toLower($key)})
            OPTIONAL MATCH path=(e)-[:RELATED_TO*1..2]-(other:Entity)
            OPTIONAL MATCH (e)<-[:MENTIONS]-(direct:Chunk)
            OPTIONAL MATCH (other)<-[:MENTIONS]-(expanded:Chunk)
            RETURN e{.*} AS entity,
                   collect(DISTINCT other{
                       .*, hops: length(path),
                       relations: [relation IN relationships(path) | relation.kind]
                   })[..$limit] AS neighbors,
                   (collect(DISTINCT direct{
                       .id, .document_id, .filename, .chunk_index,
                       .section_title, .page_number
                   }) + collect(DISTINCT expanded{
                       .id, .document_id, .filename, .chunk_index,
                       .section_title, .page_number
                   }))[..$limit] AS chunks
            """,
            key=entity_key,
            limit=max(1, min(limit, 100)),
            database_=self.neo4j_database,
        )
        return dict(records[0]) if records else {"entity": None, "neighbors": [], "chunks": []}

    async def relationships(
        self,
        source_key: str = "",
        target_key: str = "",
        limit: int = 50,
    ) -> list[dict[str, object]]:
        if not self._initialized and not await self.initialize():
            return []
        return await asyncio.to_thread(
            self._relationships_sync, source_key, target_key, limit
        )

    def _relationships_sync(
        self,
        source_key: str,
        target_key: str,
        limit: int,
    ) -> list[dict[str, object]]:
        records, _, _ = self._get_driver().execute_query(
            """
            MATCH (a:Entity)-[r:RELATED_TO]-(b:Entity)
            WHERE ($source = '' OR a.key = toLower($source) OR b.key = toLower($source))
              AND ($target = '' OR a.key = toLower($target) OR b.key = toLower($target))
            RETURN DISTINCT a{.key, .name, .kind} AS source,
                            r.kind AS relation,
                            b{.key, .name, .kind} AS target,
                            r.document_id AS document_id
            LIMIT $limit
            """,
            source=source_key,
            target=target_key,
            limit=max(1, min(limit, 200)),
            database_=self.neo4j_database,
        )
        return [dict(record) for record in records]

    async def delete_document(self, document_id: str) -> bool:
        if not self._initialized and not await self.initialize():
            return False
        try:
            await asyncio.to_thread(self._delete_document_sync, document_id)
            return True
        except (Neo4jError, ServiceUnavailable, OSError):
            return False

    def _delete_document_sync(self, document_id: str) -> None:
        self._get_driver().execute_query(
            "MATCH (c:Chunk {document_id: $id}) DETACH DELETE c",
            id=document_id,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH (s:Section {document_id: $id}) DETACH DELETE s",
            id=document_id,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH (d:Document {id: $id}) DETACH DELETE d",
            id=document_id,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH ()-[r:RELATED_TO {document_id: $id}]->() DELETE r",
            id=document_id,
            database_=self.neo4j_database,
        )
        self._delete_orphan_entities_sync()

    async def delete_memory(self, memory_id: str) -> bool:
        if not self._initialized and not await self.initialize():
            return False
        try:
            await asyncio.to_thread(self._delete_memory_sync, memory_id)
            return True
        except (Neo4jError, ServiceUnavailable, OSError):
            return False

    def _delete_memory_sync(self, memory_id: str) -> None:
        self._get_driver().execute_query(
            "MATCH (m:Memory {id: $id}) DETACH DELETE m",
            id=memory_id,
            database_=self.neo4j_database,
        )
        self._get_driver().execute_query(
            "MATCH (u:User) WHERE NOT (u)<-[:BELONGS_TO]-(:Memory) DETACH DELETE u",
            database_=self.neo4j_database,
        )

    def _delete_orphan_entities_sync(self) -> None:
        self._get_driver().execute_query(
            "MATCH (e:Entity) WHERE NOT (e)<-[:MENTIONS]-(:Chunk) DETACH DELETE e",
            database_=self.neo4j_database,
        )

    async def close(self) -> None:
        if self._driver is not None:
            await asyncio.to_thread(self._driver.close)


def normalize_graph_facts(
    entities: Iterable[dict[str, Any]],
) -> list[dict[str, str]]:
    """Normalize model-extracted entities before they can reach Cypher parameters."""
    normalized: list[dict[str, str]] = []
    seen: set[str] = set()
    for entity in entities:
        name = str(entity.get("name", "")).strip()[:120]
        if not name:
            continue
        key = re.sub(r"\s+", " ", name).casefold()
        if key in seen:
            continue
        seen.add(key)
        normalized.append(
            {"key": key, "name": name, "kind": str(entity.get("kind", "entity"))[:40]}
        )
    return normalized
