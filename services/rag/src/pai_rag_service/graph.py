"""Entity graph: SurrealDB, one store per project, reached through a connection string so
the same code serves an embedded `surrealkv://` dir or a `ws://` server. Record keys hash the
normalised name; edges use `RELATE` with a fixed id; failures raise `GraphError` to swallow."""

from __future__ import annotations

import hashlib
import logging
import threading
import unicodedata
from dataclasses import dataclass
from typing import Any

from pai_rag_service.config import GraphConfig
from pai_rag_service.errors import GraphError

__all__ = ["Edge", "Entity", "GraphStore", "Neighbor", "entity_key"]

log = logging.getLogger(__name__)


@dataclass(slots=True)
class Entity:
    """An entity pulled out of a document."""

    name: str
    kind: str = ""
    #: Chunks that mentioned it. An entity with no chunk leads the reader nowhere.
    chunk_ids: tuple[int, ...] = ()


@dataclass(slots=True)
class Edge:
    """A relation between two entities, named by `kind`."""

    source: str
    target: str
    kind: str = "related"
    weight: float = 1.0


@dataclass(slots=True)
class Neighbor:
    """A neighbouring entity and the edge that reached it."""

    name: str
    kind: str
    via: str
    weight: float


def entity_key(name: str) -> str:
    """An entity's record key: a hash of the NFC-normalised, case-folded name, so identical-looking
    spellings stay one entity.
    """
    plain = unicodedata.normalize("NFC", name).strip().casefold()
    return hashlib.blake2b(plain.encode("utf-8"), digest_size=16).hexdigest()


def _edge_key(source: str, target: str, kind: str) -> str:
    raw = f"{entity_key(source)}|{kind}|{entity_key(target)}"
    return hashlib.blake2b(raw.encode("utf-8"), digest_size=16).hexdigest()


class GraphStore:
    """One project's graph, opened lazily: an embedded store holds an exclusive lock, so opening
    early is holding a lock nobody uses.
    """

    def __init__(self, config: GraphConfig, url: str, database: str) -> None:
        self.config = config
        self.url = url
        self.database = database
        self._lock = threading.RLock()
        self._db: Any | None = None

    # -- lifecycle ---------------------------------------------------------------------

    def _connect(self) -> Any:
        with self._lock:
            if self._db is not None:
                return self._db
            try:
                from surrealdb import Surreal
            except ImportError as err:  # pragma: no cover - a pinned dependency
                raise GraphError(f"missing the `surrealdb` package: {err}") from err
            try:
                db = Surreal(self.url)
                db.use(self.config.namespace, self.database)
            except Exception as err:
                raise GraphError(
                    f"could not open the graph store at `{self.url}`: {err}. An embedded store is "
                    "usually held by another pai-rag process."
                ) from err
            self._db = db
            self._schema(db)
            return db

    def _schema(self, db: Any) -> None:
        """Tables and indexes, re-run on every open (`IF NOT EXISTS` makes that free); `TYPE
        RELATION` is what makes arrow traversal work.
        """
        for statement in (
            "DEFINE TABLE IF NOT EXISTS entity SCHEMALESS;",
            "DEFINE TABLE IF NOT EXISTS chunk SCHEMALESS;",
            "DEFINE TABLE IF NOT EXISTS mentions TYPE RELATION IN entity OUT chunk;",
            "DEFINE TABLE IF NOT EXISTS related TYPE RELATION IN entity OUT entity;",
            "DEFINE INDEX IF NOT EXISTS entity_name ON entity FIELDS name;",
            "DEFINE INDEX IF NOT EXISTS chunk_doc ON chunk FIELDS document_id;",
        ):
            try:
                db.query(statement)
            except Exception as err:
                raise GraphError(f"could not define the graph schema: {err}") from err

    def close(self) -> None:
        with self._lock:
            db, self._db = self._db, None
        if db is None:
            return
        try:
            db.close()
        except Exception as err:
            log.debug("error while closing the graph store: %s", err)

    def health(self) -> bool:
        """Whether the store opens and answers. Never raises - `doctor` prints a state."""
        try:
            self._query("RETURN 1;")
            return True
        except Exception as err:
            log.debug("graph unavailable: %s", err)
            return False

    # -- writing -----------------------------------------------------------------------

    def _query(self, sql: str, params: dict[str, Any] | None = None) -> Any:
        """One statement per call: the SDK only error-checks the first statement, so a later one
        fails silently.
        """
        db = self._connect()
        with self._lock:
            try:
                return db.query(sql, params or {})
            except Exception as err:
                raise GraphError(f"graph query failed: {err}") from err

    def upsert_entities(self, entities: list[Entity], *, document_id: str) -> None:
        """Write entities, and a `mentions` edge to every chunk that named them."""
        rows = [
            {
                "key": entity_key(item.name),
                "name": item.name,
                "kind": item.kind,
                "chunks": list(item.chunk_ids),
            }
            for item in entities
            if item.name.strip()
        ]
        if not rows:
            return
        self._query(
            """
            FOR $row IN $rows {
                LET $node = type::thing('entity', $row.key);
                UPSERT $node SET name = $row.name, kind = $row.kind;
                FOR $cid IN $row.chunks {
                    LET $chunk = type::thing('chunk', $cid);
                    LET $edge = type::thing(
                        'mentions', string::concat($row.key, '_', <string> $cid)
                    );
                    UPSERT $chunk SET document_id = $doc;
                    RELATE $node->mentions->$chunk SET id = $edge;
                };
            };
            """,
            {"rows": rows, "doc": document_id},
        )

    def relate(self, edges: list[Edge], *, document_id: str) -> None:
        """Write relations between entities; the edge id hashes (source, kind, target), so rewriting
        one overwrites instead of adding a parallel edge.

        Every edge keeps the set of documents it was read from. Without it, deleting a document
        leaves its relations behind forever - an edge names two entities and nothing else. A set,
        not a field: two documents can state the same relation, and removing one must not erase it.
        """
        rows = [
            {
                "key": _edge_key(edge.source, edge.target, edge.kind),
                "source": entity_key(edge.source),
                "target": entity_key(edge.target),
                "kind": edge.kind,
                "weight": float(edge.weight),
            }
            for edge in edges
            if edge.source.strip() and edge.target.strip()
        ]
        if not rows:
            return
        self._query(
            """
            FOR $row IN $rows {
                LET $source = type::thing('entity', $row.source);
                LET $target = type::thing('entity', $row.target);
                LET $edge = type::thing('related', $row.key);
                RELATE $source->related->$target
                    SET id = $edge, kind = $row.kind, weight = $row.weight;
                UPDATE $edge SET documents = array::union(documents ?? [], [$doc]);
            };
            """,
            {"rows": rows, "doc": document_id},
        )

    def remove_document(self, document_id: str) -> None:
        """Drop a document's trace - its chunks and every edge touching them; entities survive
        unless orphaned, since several documents usually name the same one.
        """
        self._query(
            "DELETE mentions WHERE out IN (SELECT VALUE id FROM chunk WHERE document_id = $doc);",
            {"doc": document_id},
        )
        self._query("DELETE chunk WHERE document_id = $doc;", {"doc": document_id})
        self._query(
            "UPDATE related SET documents = array::complement(documents ?? [], [$doc]);",
            {"doc": document_id},
        )
        self._query("DELETE related WHERE array::len(documents ?? []) = 0;")
        self._query(
            "DELETE entity WHERE count(->mentions) = 0 "
            "AND count(->related) = 0 AND count(<-related) = 0;"
        )

    def drop(self) -> None:
        """Empty the graph, keep the schema. For rebuilding a whole library."""
        for table in ("mentions", "related", "chunk", "entity"):
            self._query(f"DELETE {table};")

    # -- reading -----------------------------------------------------------------------

    def neighbors(self, name: str, *, limit: int = 20) -> list[Neighbor]:
        """Entities directly linked to `name`, both directions; one hop only, because two hops out a
        prose-derived graph connects nearly everything.
        """
        me = _record("entity", entity_key(name))
        out = self._query(
            "SELECT out.name AS name, out.kind AS kind, kind AS via, weight "
            "FROM related WHERE in = $me LIMIT $limit;",
            {"me": me, "limit": limit},
        )
        back = self._query(
            "SELECT in.name AS name, in.kind AS kind, kind AS via, weight "
            "FROM related WHERE out = $me LIMIT $limit;",
            {"me": me, "limit": limit},
        )
        seen: dict[str, Neighbor] = {}
        for row in [*_rows(out), *_rows(back)]:
            if not isinstance(row, dict):
                continue
            label = str(row.get("name") or "")
            if not label or label in seen:
                continue
            seen[label] = Neighbor(
                name=label,
                kind=str(row.get("kind") or ""),
                via=str(row.get("via") or "related"),
                weight=float(row.get("weight") or 0.0),
            )
        return list(seen.values())[:limit]

    def chunks_for(self, names: list[str], *, limit: int = 50) -> list[int]:
        """Chunks where these entities were mentioned - the only bridge from graph back to
        retrieval, returning the same result kind as vector and keyword search.
        """
        ids = [_record("entity", entity_key(item)) for item in names if item.strip()]
        if not ids:
            return []
        found = self._query(
            "SELECT VALUE out FROM mentions WHERE in IN $ids LIMIT $limit;",
            {"ids": ids, "limit": limit},
        )
        chunks: list[int] = []
        for row in _rows(found):
            parsed = _chunk_id(row.get("out") if isinstance(row, dict) else row)
            if parsed is not None and parsed not in chunks:
                chunks.append(parsed)
        return chunks

    def count(self) -> tuple[int, int]:
        """(entities, relations); raises rather than returning `(0, 0)`, which would report an empty
        graph for a broken store.
        """
        entities = _first_count(self._query("SELECT count() FROM entity GROUP ALL;"))
        relations = _first_count(self._query("SELECT count() FROM related GROUP ALL;"))
        return entities, relations


def _record(table: str, key: str) -> Any:
    """A bound record id. `in IN $ids` compares record ids; a string never matches one."""
    from surrealdb import RecordID

    return RecordID(table, key)


def _rows(result: Any) -> list[Any]:
    """Flatten what the SDK hands back into a list of rows."""
    if result is None:
        return []
    if isinstance(result, dict):
        return [result]
    if not isinstance(result, list):
        return [result]
    if result and isinstance(result[0], list):
        return [row for group in result for row in group]
    return result


def _first_count(result: Any) -> int:
    for row in _rows(result):
        if isinstance(row, dict) and "count" in row:
            return int(row["count"])
    return 0


def _chunk_id(raw: Any) -> int | None:
    """A chunk id out of a `chunk:123` record id, whatever shape the SDK hands back."""
    if raw is None:
        return None
    if isinstance(raw, int):
        return raw
    identifier = getattr(raw, "id", None)
    if isinstance(identifier, int):
        return identifier
    text = str(identifier if identifier is not None else raw)
    tail = text.rsplit(":", 1)[-1].strip("⟨⟩`")
    try:
        return int(tail)
    except ValueError:
        return None
