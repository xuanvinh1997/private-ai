"""Metadata store: SQLite for documents, chunks and the keyword index; vectors live in
Qdrant. FTS5 uses external content and `remove_diacritics 2` (Vietnamese is often typed
unaccented); path is identity, mtime+size the fingerprint, and deletes are explicit."""

from __future__ import annotations

import json
import re
import sqlite3
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from pai_rag_service.chunking import Chunk

__all__ = [
    "ChunkRow",
    "DocumentRow",
    "META_EMBEDDER",
    "META_EMBED_INPUT",
    "META_EXTRACT",
    "SCHEMA_VERSION",
    "Store",
]

SCHEMA_VERSION = 1

META_EMBEDDER = "embedder.id"
META_EMBEDDER_DIM = "embedder.dim"
META_EMBED_INPUT = "embed.input.version"
META_EXTRACT = "extract.version"
#: Files seen by the last scan, files skipped at the cap, and when it finished; stored so the UI can say when the last scan ran before this session scans.
META_SCAN_FILES = "scan.files"
META_SCAN_SKIPPED = "scan.skipped"
META_SCAN_AT = "scan.at"

SCHEMA = """
CREATE TABLE IF NOT EXISTS documents (
  id        TEXT PRIMARY KEY,
  path      TEXT NOT NULL UNIQUE,
  title     TEXT NOT NULL,
  format    TEXT NOT NULL,
  bytes     INTEGER NOT NULL,
  mtime     INTEGER NOT NULL,
  pages     INTEGER NOT NULL DEFAULT 0,
  ocr_pages TEXT NOT NULL DEFAULT '[]',
  added_at  INTEGER NOT NULL,
  error     TEXT
);

CREATE TABLE IF NOT EXISTS chunks (
  id          INTEGER PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  ordinal     INTEGER NOT NULL,
  section     TEXT NOT NULL DEFAULT '',
  page        INTEGER NOT NULL DEFAULT 0,
  body        TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS chunks_by_document ON chunks (document_id, ordinal);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
  body, section, content = 'chunks', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts (rowid, body, section) VALUES (new.id, new.body, new.section);
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, section)
  VALUES ('delete', old.id, old.body, old.section);
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
  INSERT INTO chunks_fts (chunks_fts, rowid, body, section)
  VALUES ('delete', old.id, old.body, old.section);
  INSERT INTO chunks_fts (rowid, body, section) VALUES (new.id, new.body, new.section);
END;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- Files that were tried and could not be read, with the fingerprint at the time; without this table every scan re-extracts the same broken files.
-- Files still in the project folder that the user removed from the library; without this the next scan re-ingests exactly what they just removed.
CREATE TABLE IF NOT EXISTS excluded (
  path TEXT PRIMARY KEY,
  at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS failures (
  path   TEXT PRIMARY KEY,
  mtime  INTEGER NOT NULL,
  size   INTEGER NOT NULL,
  reason TEXT NOT NULL
);
"""


@dataclass(slots=True)
class DocumentRow:
    id: str
    path: str
    title: str
    format: str
    bytes: int
    mtime: int
    pages: int
    ocr_pages: list[int]
    added_at: int
    error: str | None
    chunks: int


@dataclass(slots=True)
class ChunkRow:
    id: int
    document_id: str
    title: str
    path: str
    ordinal: int
    section: str
    page: int
    body: str


def _fts_expressions(query: str) -> tuple[str, str] | None:
    """`(AND expression, OR expression)` from a user query; user text is never spliced into `MATCH` syntax, so tokens are extracted and quoted into literals."""
    tokens = [f'"{token}"' for token in re.findall(r"[^\W_]+", query, re.UNICODE)]
    if not tokens:
        return None
    return " AND ".join(tokens), " OR ".join(tokens)


class Store:
    """One SQLite file per document project."""

    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        # `isolation_level=None` = autocommit per statement with hand-rolled transactions; the implicit mode leaves one open until the next `commit()`.
        self.conn = sqlite3.connect(str(path), check_same_thread=False, isolation_level=None)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute("PRAGMA journal_mode = WAL")
        self.conn.execute("PRAGMA foreign_keys = ON")
        self.conn.execute("PRAGMA synchronous = NORMAL")
        self.conn.executescript(SCHEMA)
        self.conn.commit()

    def close(self) -> None:
        # Checkpoint the WAL on close, or the directory keeps a `-wal` file the next open must replay.
        try:
            self.conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        finally:
            self.conn.close()

    def __enter__(self) -> Store:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- meta ---------------------------------------------------------------------------

    def meta(self, key: str) -> str | None:
        row = self.conn.execute("SELECT value FROM meta WHERE key = ?", (key,)).fetchone()
        return row["value"] if row else None

    def set_meta(self, key: str, value: str) -> None:
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )

    def identity(self) -> dict[str, str | None]:
        return {
            "embedder": self.meta(META_EMBEDDER),
            "dim": self.meta(META_EMBEDDER_DIM),
            "embed_input": self.meta(META_EMBED_INPUT),
            "extract": self.meta(META_EXTRACT),
        }

    def set_identity(
        self, *, embedder: str, dim: int | None, embed_input: int, extract: int
    ) -> None:
        self.set_meta(META_EMBEDDER, embedder)
        if dim is not None:
            self.set_meta(META_EMBEDDER_DIM, str(dim))
        self.set_meta(META_EMBED_INPUT, str(embed_input))
        self.set_meta(META_EXTRACT, str(extract))

    # -- fingerprints -------------------------------------------------------------------

    def known_files(self) -> dict[str, tuple[int, int]]:
        """`path -> (mtime, size)` for every file ingested or tried and failed; one lookup for both tables, since the next scan must skip both."""
        out: dict[str, tuple[int, int]] = {}
        for row in self.conn.execute("SELECT path, mtime, bytes FROM documents"):
            out[row["path"]] = (row["mtime"], row["bytes"])
        for row in self.conn.execute("SELECT path, mtime, size FROM failures"):
            out.setdefault(row["path"], (row["mtime"], row["size"]))
        return out

    def put_failure(self, path: str, mtime: int, size: int, reason: str) -> None:
        self.conn.execute(
            "INSERT INTO failures (path, mtime, size, reason) VALUES (?, ?, ?, ?) "
            "ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime, "
            "size = excluded.size, reason = excluded.reason",
            (path, mtime, size, reason),
        )

    def clear_failure(self, path: str) -> None:
        self.conn.execute("DELETE FROM failures WHERE path = ?", (path,))

    def failures(self) -> list[tuple[str, str]]:
        rows = self.conn.execute("SELECT path, reason FROM failures ORDER BY path")
        return [(row["path"], row["reason"]) for row in rows]

    def forget_fingerprints(self) -> int:
        """Forget every fingerprint so the next scan re-reads the whole folder; called when the extractor or the embed input changed."""
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            changed = cur.execute("UPDATE documents SET mtime = 0").rowcount
            cur.execute("DELETE FROM failures")
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        return changed

    # -- documents ----------------------------------------------------------------------

    def put_document(
        self,
        *,
        doc_id: str,
        path: str,
        title: str,
        fmt: str,
        size: int,
        mtime: int,
        pages: int,
        ocr_pages: list[int],
        chunks: list[Chunk],
    ) -> list[int]:
        """Write a document and all its chunks, returning chunk ids in order; an existing document is fully replaced rather than merged."""
        now = int(time.time() * 1000)
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            self._forget_chunks(cur, doc_id)
            cur.execute(
                "INSERT INTO documents (id, path, title, format, bytes, mtime, pages, "
                "ocr_pages, added_at, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL) "
                "ON CONFLICT(id) DO UPDATE SET path = excluded.path, title = excluded.title, "
                "format = excluded.format, bytes = excluded.bytes, mtime = excluded.mtime, "
                "pages = excluded.pages, ocr_pages = excluded.ocr_pages, error = NULL",
                (doc_id, path, title, fmt, size, mtime, pages, json.dumps(ocr_pages), now),
            )
            ids: list[int] = []
            for chunk in chunks:
                cur.execute(
                    "INSERT INTO chunks (document_id, ordinal, section, page, body) "
                    "VALUES (?, ?, ?, ?, ?)",
                    (doc_id, chunk.ordinal, chunk.section, chunk.page, chunk.text),
                )
                ids.append(int(cur.lastrowid))
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        self.conn.execute("DELETE FROM failures WHERE path = ?", (path,))
        return ids

    @staticmethod
    def _forget_chunks(cur: sqlite3.Cursor, doc_id: str) -> None:
        """Delete a document's chunks. Explicit - see decision 4 at the top of the file."""
        cur.execute("DELETE FROM chunks WHERE document_id = ?", (doc_id,))

    def remove_document(self, doc_id: str) -> list[int]:
        """Delete a document, returning the deleted chunk ids so the caller can clean Qdrant."""
        rows = self.conn.execute("SELECT id FROM chunks WHERE document_id = ?", (doc_id,))
        ids = [int(row["id"]) for row in rows]
        cur = self.conn.cursor()
        cur.execute("BEGIN")
        try:
            self._forget_chunks(cur, doc_id)
            cur.execute("DELETE FROM documents WHERE id = ?", (doc_id,))
            cur.execute("COMMIT")
        except Exception:
            cur.execute("ROLLBACK")
            raise
        return ids

    def documents(self) -> list[DocumentRow]:
        rows = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d ORDER BY d.added_at DESC"
        )
        return [self._document(row) for row in rows]

    def document(self, doc_id: str) -> DocumentRow | None:
        row = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d WHERE d.id = ?",
            (doc_id,),
        ).fetchone()
        return self._document(row) if row else None

    def document_by_path(self, path: str) -> DocumentRow | None:
        row = self.conn.execute(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n "
            "FROM documents d WHERE d.path = ?",
            (path,),
        ).fetchone()
        return self._document(row) if row else None

    @staticmethod
    def _document(row: sqlite3.Row) -> DocumentRow:
        return DocumentRow(
            id=row["id"],
            path=row["path"],
            title=row["title"],
            format=row["format"],
            bytes=row["bytes"],
            mtime=row["mtime"],
            pages=row["pages"],
            ocr_pages=json.loads(row["ocr_pages"] or "[]"),
            added_at=row["added_at"],
            error=row["error"],
            chunks=row["n"],
        )

    # -- chunks -------------------------------------------------------------------------

    _CHUNK_SELECT = (
        "SELECT c.id, c.document_id, c.ordinal, c.section, c.page, c.body, "
        "d.title, d.path FROM chunks c JOIN documents d ON d.id = c.document_id "
    )

    def chunks_by_id(self, ids: list[int]) -> list[ChunkRow]:
        if not ids:
            return []
        marks = ",".join("?" * len(ids))
        rows = self.conn.execute(f"{self._CHUNK_SELECT} WHERE c.id IN ({marks})", ids)
        return [self._chunk(row) for row in rows]

    def chunks_of(self, doc_id: str, offset: int = 0, limit: int = 50) -> list[ChunkRow]:
        rows = self.conn.execute(
            f"{self._CHUNK_SELECT} WHERE c.document_id = ? ORDER BY c.ordinal LIMIT ? OFFSET ?",
            (doc_id, limit, offset),
        )
        return [self._chunk(row) for row in rows]

    @staticmethod
    def _chunk(row: sqlite3.Row) -> ChunkRow:
        return ChunkRow(
            id=row["id"],
            document_id=row["document_id"],
            title=row["title"],
            path=row["path"],
            ordinal=row["ordinal"],
            section=row["section"],
            page=row["page"],
            body=row["body"],
        )

    def counts(self) -> tuple[int, int]:
        docs = self.conn.execute("SELECT COUNT(*) AS n FROM documents").fetchone()["n"]
        chunks = self.conn.execute("SELECT COUNT(*) AS n FROM chunks").fetchone()["n"]
        return int(docs), int(chunks)

    # -- keyword search -----------------------------------------------------------------

    def search_keyword(self, query: str, limit: int) -> list[int]:
        """Chunk ids in BM25 order, best first; `section` is weighted twice `body`, since a query matching a heading is usually about that section."""
        built = _fts_expressions(query)
        if built is None:
            return []
        strict, loose = built
        sql = (
            "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ? "
            "ORDER BY bm25(chunks_fts, 1.0, 2.0) LIMIT ?"
        )
        hits = [int(row[0]) for row in self.conn.execute(sql, (strict, limit))]
        if not hits:
            # Requiring every term is the right filter when it returns anything; when it returns nothing - a whole sentence was typed - any term beats none.
            hits = [int(row[0]) for row in self.conn.execute(sql, (loose, limit))]
        return hits

    # -- exclusion ----------------------------------------------------------------------

    def exclude(self, path: str, at: int) -> None:
        """Mark a file as removed from the library by the user."""
        self.conn.execute(
            "INSERT INTO excluded (path, at) VALUES (?, ?) "
            "ON CONFLICT(path) DO UPDATE SET at = excluded.at",
            (path, at),
        )

    def allow(self, path: str) -> None:
        """Clear the exclusion mark - the user re-ingested this file explicitly."""
        self.conn.execute("DELETE FROM excluded WHERE path = ?", (path,))

    def excluded(self) -> set[str]:
        return {row["path"] for row in self.conn.execute("SELECT path FROM excluded")}

    def clear_excluded(self) -> int:
        """Allow every file again. Used when the user reprocesses the whole library."""
        cur = self.conn.execute("DELETE FROM excluded")
        return cur.rowcount

    def integrity(self) -> None:
        """Raise if the FTS index has drifted from the content table. Used by `pai-rag doctor`."""
        self.conn.execute("INSERT INTO chunks_fts (chunks_fts) VALUES ('integrity-check')")

    def stats(self) -> dict[str, Any]:
        docs, chunks = self.counts()
        return {
            "documents": docs,
            "chunks": chunks,
            "failures": len(self.failures()),
            **self.identity(),
        }
