use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Chunk;

pub const SCHEMA_VERSION: u32 = 1;
pub const META_EMBEDDER: &str = "embedder.id";
pub const META_EMBEDDER_DIM: &str = "embedder.dim";
pub const META_EMBED_INPUT: &str = "embed.input.version";
pub const META_EXTRACT: &str = "extract.version";
pub const META_SCAN_FILES: &str = "scan.files";
pub const META_SCAN_SKIPPED: &str = "scan.skipped";
pub const META_SCAN_AT: &str = "scan.at";

const SCHEMA: &str = r#"
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
"#;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("cannot prepare RAG store directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("system clock is before the Unix epoch")]
    Clock,
    #[error("cannot encode OCR page metadata: {0}")]
    Json(#[from] serde_json::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRow {
    pub id: String,
    pub path: String,
    pub title: String,
    pub format: String,
    pub bytes: i64,
    pub mtime: i64,
    pub pages: i64,
    pub ocr_pages: Vec<u32>,
    pub added_at: i64,
    pub error: Option<String>,
    pub chunks: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRow {
    pub id: i64,
    pub document_id: String,
    pub title: String,
    pub path: String,
    pub ordinal: i64,
    pub section: String,
    pub page: i64,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct DocumentInput<'a> {
    pub id: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub format: &'a str,
    pub bytes: i64,
    pub mtime: i64,
    pub pages: i64,
    pub ocr_pages: &'a [u32],
    pub chunks: &'a [Chunk],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub embedder: Option<String>,
    pub dim: Option<String>,
    pub embed_input: Option<String>,
    pub extract: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub documents: i64,
    pub chunks: i64,
    pub failures: usize,
    #[serde(flatten)]
    pub identity: Identity,
}

/// SQLite metadata and FTS store compatible with libraries created before the Rust migration.
pub struct Store {
    path: Option<PathBuf>,
    connection: Connection,
}

impl Drop for Store {
    fn drop(&mut self) {
        // Match the Python store: leave no WAL for the next process to replay when possible.
        let _ = self
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        initialize(&connection)?;
        Ok(Self {
            path: Some(path.to_owned()),
            connection,
        })
    }

    pub fn in_memory() -> StoreResult<Self> {
        let connection = Connection::open_in_memory()?;
        initialize(&connection)?;
        Ok(Self {
            path: None,
            connection,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn checkpoint(&self) -> StoreResult<()> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> StoreResult<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn identity(&self) -> StoreResult<Identity> {
        Ok(Identity {
            embedder: self.meta(META_EMBEDDER)?,
            dim: self.meta(META_EMBEDDER_DIM)?,
            embed_input: self.meta(META_EMBED_INPUT)?,
            extract: self.meta(META_EXTRACT)?,
        })
    }

    pub fn set_identity(
        &self,
        embedder: &str,
        dim: Option<usize>,
        embed_input: u32,
        extract: u32,
    ) -> StoreResult<()> {
        self.set_meta(META_EMBEDDER, embedder)?;
        if let Some(dim) = dim {
            self.set_meta(META_EMBEDDER_DIM, &dim.to_string())?;
        }
        self.set_meta(META_EMBED_INPUT, &embed_input.to_string())?;
        self.set_meta(META_EXTRACT, &extract.to_string())
    }

    pub fn known_files(&self) -> StoreResult<BTreeMap<String, (i64, i64)>> {
        let mut output = BTreeMap::new();
        let mut documents = self
            .connection
            .prepare("SELECT path, mtime, bytes FROM documents")?;
        for row in documents.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })? {
            let (path, mtime, bytes) = row?;
            output.insert(path, (mtime, bytes));
        }
        let mut failures = self
            .connection
            .prepare("SELECT path, mtime, size FROM failures")?;
        for row in failures.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
        })? {
            let (path, mtime, size) = row?;
            output.entry(path).or_insert((mtime, size));
        }
        Ok(output)
    }

    pub fn put_failure(&self, path: &str, mtime: i64, size: i64, reason: &str) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO failures (path, mtime, size, reason) VALUES (?, ?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime, \
             size = excluded.size, reason = excluded.reason",
            params![path, mtime, size, reason],
        )?;
        Ok(())
    }

    pub fn clear_failure(&self, path: &str) -> StoreResult<()> {
        self.connection
            .execute("DELETE FROM failures WHERE path = ?", [path])?;
        Ok(())
    }

    pub fn failures(&self) -> StoreResult<Vec<(String, String)>> {
        let mut statement = self
            .connection
            .prepare("SELECT path, reason FROM failures ORDER BY path")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn forget_fingerprints(&mut self) -> StoreResult<usize> {
        let transaction = self.connection.transaction()?;
        let changed = transaction.execute("UPDATE documents SET mtime = 0", [])?;
        transaction.execute("DELETE FROM failures", [])?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn put_document(&mut self, input: &DocumentInput<'_>) -> StoreResult<Vec<i64>> {
        let now = now_millis()?;
        let ocr_pages = serde_json::to_string(input.ocr_pages)?;
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM chunks WHERE document_id = ?", [input.id])?;
        transaction.execute(
            "INSERT INTO documents (id, path, title, format, bytes, mtime, pages, \
             ocr_pages, added_at, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL) \
             ON CONFLICT(id) DO UPDATE SET path = excluded.path, title = excluded.title, \
             format = excluded.format, bytes = excluded.bytes, mtime = excluded.mtime, \
             pages = excluded.pages, ocr_pages = excluded.ocr_pages, error = NULL",
            params![
                input.id,
                input.path,
                input.title,
                input.format,
                input.bytes,
                input.mtime,
                input.pages,
                ocr_pages,
                now,
            ],
        )?;
        let mut ids = Vec::with_capacity(input.chunks.len());
        for chunk in input.chunks {
            transaction.execute(
                "INSERT INTO chunks (document_id, ordinal, section, page, body) \
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    input.id,
                    chunk.ordinal as i64,
                    chunk.section,
                    i64::from(chunk.page),
                    chunk.text,
                ],
            )?;
            ids.push(transaction.last_insert_rowid());
        }
        transaction.commit()?;
        self.clear_failure(input.path)?;
        Ok(ids)
    }

    pub fn remove_document(&mut self, document_id: &str) -> StoreResult<Vec<i64>> {
        let ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM chunks WHERE document_id = ?")?;
            statement
                .query_map([document_id], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM chunks WHERE document_id = ?", [document_id])?;
        transaction.execute("DELETE FROM documents WHERE id = ?", [document_id])?;
        transaction.commit()?;
        Ok(ids)
    }

    pub fn documents(&self) -> StoreResult<Vec<DocumentRow>> {
        let mut statement = self.connection.prepare(
            "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n \
             FROM documents d ORDER BY d.added_at DESC",
        )?;
        let rows = statement.query_map([], document_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn document(&self, document_id: &str) -> StoreResult<Option<DocumentRow>> {
        Ok(self
            .connection
            .query_row(
                "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n \
                 FROM documents d WHERE d.id = ?",
                [document_id],
                document_from_row,
            )
            .optional()?)
    }

    pub fn document_by_path(&self, path: &str) -> StoreResult<Option<DocumentRow>> {
        Ok(self
            .connection
            .query_row(
                "SELECT d.*, (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id) AS n \
                 FROM documents d WHERE d.path = ?",
                [path],
                document_from_row,
            )
            .optional()?)
    }

    pub fn chunks_by_id(&self, ids: &[i64]) -> StoreResult<Vec<ChunkRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("{} WHERE c.id IN ({marks})", chunk_select());
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(ids), chunk_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn chunks_of(
        &self,
        document_id: &str,
        offset: usize,
        limit: usize,
    ) -> StoreResult<Vec<ChunkRow>> {
        let sql = format!(
            "{} WHERE c.document_id = ? ORDER BY c.ordinal LIMIT ? OFFSET ?",
            chunk_select()
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![document_id, limit as i64, offset as i64],
            chunk_from_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every chunk in stable document/ordinal order, used by the embedding catch-up pass.
    pub fn all_chunks(&self) -> StoreResult<Vec<ChunkRow>> {
        let sql = format!("{} ORDER BY c.document_id, c.ordinal", chunk_select());
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], chunk_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn counts(&self) -> StoreResult<(i64, i64)> {
        let documents = self
            .connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
        let chunks = self
            .connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok((documents, chunks))
    }

    pub fn search_keyword(&self, query: &str, limit: usize) -> StoreResult<Vec<i64>> {
        let Some((strict, loose)) = fts_expressions(query) else {
            return Ok(Vec::new());
        };
        let sql = "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ? \
                   ORDER BY bm25(chunks_fts, 1.0, 2.0) LIMIT ?";
        let search = |expression: &str| -> Result<Vec<i64>, rusqlite::Error> {
            let mut statement = self.connection.prepare(sql)?;
            statement
                .query_map(params![expression, limit as i64], |row| row.get(0))?
                .collect()
        };
        let hits = search(&strict)?;
        if hits.is_empty() {
            Ok(search(&loose)?)
        } else {
            Ok(hits)
        }
    }

    pub fn exclude(&self, path: &str, at: i64) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO excluded (path, at) VALUES (?, ?) \
             ON CONFLICT(path) DO UPDATE SET at = excluded.at",
            params![path, at],
        )?;
        Ok(())
    }

    pub fn allow(&self, path: &str) -> StoreResult<()> {
        self.connection
            .execute("DELETE FROM excluded WHERE path = ?", [path])?;
        Ok(())
    }

    pub fn excluded(&self) -> StoreResult<BTreeSet<String>> {
        let mut statement = self.connection.prepare("SELECT path FROM excluded")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn clear_excluded(&self) -> StoreResult<usize> {
        Ok(self.connection.execute("DELETE FROM excluded", [])?)
    }

    pub fn integrity(&self) -> StoreResult<()> {
        self.connection.execute(
            "INSERT INTO chunks_fts (chunks_fts) VALUES ('integrity-check')",
            [],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> StoreResult<Stats> {
        let (documents, chunks) = self.counts()?;
        Ok(Stats {
            documents,
            chunks,
            failures: self.failures()?.len(),
            identity: self.identity()?,
        })
    }
}

fn initialize(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.execute_batch(SCHEMA)
}

fn now_millis() -> StoreResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::Clock)?;
    Ok(elapsed.as_millis().min(i64::MAX as u128) as i64)
}

fn document_from_row(row: &Row<'_>) -> Result<DocumentRow, rusqlite::Error> {
    let raw_pages: String = row.get("ocr_pages")?;
    let ocr_pages = serde_json::from_str(&raw_pages).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(DocumentRow {
        id: row.get("id")?,
        path: row.get("path")?,
        title: row.get("title")?,
        format: row.get("format")?,
        bytes: row.get("bytes")?,
        mtime: row.get("mtime")?,
        pages: row.get("pages")?,
        ocr_pages,
        added_at: row.get("added_at")?,
        error: row.get("error")?,
        chunks: row.get("n")?,
    })
}

fn chunk_select() -> &'static str {
    "SELECT c.id, c.document_id, c.ordinal, c.section, c.page, c.body, \
     d.title, d.path FROM chunks c JOIN documents d ON d.id = c.document_id"
}

fn chunk_from_row(row: &Row<'_>) -> Result<ChunkRow, rusqlite::Error> {
    Ok(ChunkRow {
        id: row.get("id")?,
        document_id: row.get("document_id")?,
        title: row.get("title")?,
        path: row.get("path")?,
        ordinal: row.get("ordinal")?,
        section: row.get("section")?,
        page: row.get("page")?,
        body: row.get("body")?,
    })
}

fn fts_expressions(query: &str) -> Option<(String, String)> {
    let tokens: Vec<String> = query
        .split(|character: char| character == '_' || !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some((tokens.join(" AND "), tokens.join(" OR ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SectionAwareSplitter;

    fn input<'a>(chunks: &'a [Chunk]) -> DocumentInput<'a> {
        DocumentInput {
            id: "doc-1",
            path: "docs/guide.md",
            title: "Guide",
            format: "markdown",
            bytes: 123,
            mtime: 456,
            pages: 2,
            ocr_pages: &[2],
            chunks,
        }
    }

    #[test]
    fn schema_and_fts_are_compatible_with_python_store() -> StoreResult<()> {
        let mut store = Store::in_memory()?;
        let chunks = SectionAwareSplitter::new(100, 0)
            .split("# Cài đặt\n\nHướng dẫn cấu hình dịch vụ.\n\n# Search\n\nSemantic retrieval.");
        let ids = store.put_document(&input(&chunks))?;

        assert_eq!(store.counts()?, (1, chunks.len() as i64));
        assert_eq!(store.document("doc-1")?.unwrap().ocr_pages, vec![2]);
        assert_eq!(store.search_keyword("cai dat", 10)?, vec![ids[0]]);
        assert_eq!(store.search_keyword("không-có semantic", 10)?, vec![ids[1]]);
        store.integrity()?;
        Ok(())
    }

    #[test]
    fn replacement_is_atomic_and_returns_ids_for_vector_cleanup() -> StoreResult<()> {
        let mut store = Store::in_memory()?;
        let first = SectionAwareSplitter::new(8, 0).split("alpha beta gamma");
        store.put_document(&input(&first))?;
        let replacement = SectionAwareSplitter::new(100, 0).split("replacement");
        let new_ids = store.put_document(&input(&replacement))?;

        assert_eq!(store.chunks_of("doc-1", 0, 50)?.len(), 1);
        assert_eq!(store.remove_document("doc-1")?, new_ids);
        assert_eq!(store.counts()?, (0, 0));
        Ok(())
    }

    #[test]
    fn failures_and_exclusions_follow_scan_semantics() -> StoreResult<()> {
        let mut store = Store::in_memory()?;
        store.put_failure("broken.pdf", 10, 20, "bad PDF")?;
        assert_eq!(store.known_files()?["broken.pdf"], (10, 20));
        store.exclude("manual.pdf", 99)?;
        assert!(store.excluded()?.contains("manual.pdf"));
        assert_eq!(store.clear_excluded()?, 1);

        let chunks = SectionAwareSplitter::default().split("ok");
        store.put_document(&input(&chunks))?;
        assert_eq!(store.forget_fingerprints()?, 1);
        assert_eq!(store.document("doc-1")?.unwrap().mtime, 0);
        assert!(store.failures()?.is_empty());
        Ok(())
    }

    #[test]
    fn a_persistent_store_reopens_with_the_same_python_schema() -> StoreResult<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("rag.sqlite");
        {
            let mut store = Store::open(&path)?;
            let chunks = SectionAwareSplitter::default().split("persistent body");
            store.put_document(&input(&chunks))?;
        }

        let reopened = Store::open(&path)?;
        assert_eq!(reopened.counts()?, (1, 1));
        assert_eq!(
            reopened.chunks_of("doc-1", 0, 50)?[0].body,
            "persistent body"
        );
        Ok(())
    }
}
