//! The store: SQLite + FTS5, with a graph built on the same symbol table.
//! FTS5 keeps its own content (external-content needs triggers `trusted_schema = OFF` bans),
//! schema drift rebuilds rather than refuses, and `refs`/`edges` are split by lifetime.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::error::IndexError;
use crate::extract::Extraction;
use crate::graph::{
    CentralSymbol, DirectorySummary, EdgeKind, GraphEdge, GraphNode, MODULE_KIND, Overview, Owner,
    Stats, Target,
};
use crate::symbol::{Symbol, SymbolKind};

type Result<T> = std::result::Result<T, IndexError>;

/// `'PIDX'`. Opening someone else's SQLite file is caught before anything is written.
const APPLICATION_ID: i32 = 0x50494458;
const SCHEMA_VERSION: i32 = 2;

/// How many candidates are still worth writing; past the cap the reference is dropped, since a meaningless edge is worse than none.
const MAX_CANDIDATES: usize = 4;

const SCHEMA: &str = r#"
CREATE TABLE files (
  id    INTEGER PRIMARY KEY,
  path  TEXT    NOT NULL UNIQUE,
  lang  TEXT    NOT NULL,
  mtime INTEGER NOT NULL,
  size  INTEGER NOT NULL
) STRICT;

CREATE TABLE symbols (
  id         INTEGER PRIMARY KEY,
  file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  name       TEXT    NOT NULL,
  kind       TEXT    NOT NULL,
  parent     TEXT,
  start_line INTEGER NOT NULL,
  end_line   INTEGER NOT NULL,
  signature  TEXT    NOT NULL
) STRICT;

CREATE INDEX symbols_by_file ON symbols (file_id, start_line);
CREATE INDEX symbols_by_name ON symbols (name);

CREATE VIRTUAL TABLE symbols_fts USING fts5(name, parent, signature, tokenize = 'unicode61');

CREATE TABLE refs (
  src      INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  kind     TEXT    NOT NULL,
  dst      INTEGER          REFERENCES symbols(id) ON DELETE CASCADE,
  dst_name TEXT,
  line     INTEGER NOT NULL
) STRICT;

CREATE INDEX refs_by_src ON refs (src);

CREATE TABLE edges (
  src  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  dst  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
  kind TEXT    NOT NULL,
  path TEXT    NOT NULL,
  line INTEGER NOT NULL
) STRICT;

CREATE INDEX edges_by_src ON edges (src);
CREATE INDEX edges_by_dst ON edges (dst);
CREATE UNIQUE INDEX edges_once ON edges (src, dst, kind, line);

CREATE TABLE meta (
  key   TEXT    PRIMARY KEY,
  value INTEGER NOT NULL
) STRICT;
"#;

/// Module nodes are not declarations anyone searches for, so `symbol_search` and `outline` exclude them.
const NOT_MODULE: &str = "s.kind <> 'module'";

const SELECT_SYMBOL: &str = "SELECT s.name, s.kind, s.parent, s.start_line, s.end_line, \
     s.signature, f.path FROM symbols s JOIN files f ON f.id = s.file_id";

const SELECT_NODE: &str = "SELECT s.id, s.name, s.kind, f.path, s.start_line \
     FROM symbols s JOIN files f ON f.id = s.file_id";

/// A known `files` row, enough to answer "has this file changed".
#[derive(Clone, Copy)]
pub struct FileState {
    pub id: i64,
    pub mtime: i64,
    pub size: i64,
}

pub struct Store {
    /// `Connection` is not `Sync`; a real lock beats a pool since writes are serial and always inside `spawn_blocking`.
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        Store::from_connection(Connection::open(path)?)
    }

    /// For tests, and for sessions that need not outlive this run.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Store> {
        configure(&conn)?;
        ensure_schema(&mut conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, body: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| IndexError::Unavailable("khoá kết nối bị nhiễm độc".into()))?;
        body(&mut guard)
    }

    /// Every known file with its fingerprint; fetched in one query and compared in memory.
    pub fn known_files(&self) -> Result<HashMap<String, FileState>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT id, path, mtime, size FROM files")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    FileState {
                        id: row.get(0)?,
                        mtime: row.get(2)?,
                        size: row.get(3)?,
                    },
                ))
            })?;
            let mut known = HashMap::new();
            for row in rows {
                let (path, state) = row?;
                known.insert(path, state);
            }
            Ok(known)
        })
    }

    /// Known file paths for `@` completion: `path` only, ordered for stability; scoring lives in [`crate::complete`].
    pub fn paths(&self) -> Result<Vec<String>> {
        self.with(|conn| {
            let mut stmt = conn.prepare("SELECT path FROM files ORDER BY path")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
    }

    /// Replace, not patch, a file's symbols and references in one transaction; edges follow via `ON DELETE CASCADE`.
    pub fn replace_file(
        &self,
        path: &str,
        lang: &str,
        mtime: i64,
        size: i64,
        found: &Extraction,
    ) -> Result<()> {
        self.with(|conn| {
            let tx = conn.transaction()?;
            forget_symbols_of(&tx, path)?;
            tx.execute(
                "INSERT INTO files (path, lang, mtime, size) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(path) DO UPDATE SET lang = ?2, mtime = ?3, size = ?4",
                params![path, lang, mtime, size],
            )?;
            let file_id: i64 =
                tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                    r.get(0)
                })?;

            // The module node comes first as the default owner, and stays out of FTS: people search function names.
            let module = module_name(path);
            tx.execute(
                "INSERT INTO symbols (file_id, name, kind, parent, start_line, end_line, signature) \
                 VALUES (?1, ?2, ?3, NULL, 1, 1, ?4)",
                params![file_id, module, MODULE_KIND, path],
            )?;
            let module_id = tx.last_insert_rowid();

            let mut ids = Vec::with_capacity(found.symbols.len());
            let mut by_name: HashMap<&str, i64> = HashMap::new();
            for symbol in &found.symbols {
                tx.execute(
                    "INSERT INTO symbols \
                     (file_id, name, kind, parent, start_line, end_line, signature) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        file_id,
                        symbol.name,
                        symbol.kind.as_str(),
                        symbol.parent,
                        symbol.start_line,
                        symbol.end_line,
                        symbol.signature,
                    ],
                )?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO symbols_fts (rowid, name, parent, signature) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, symbol.name, symbol.parent, symbol.signature],
                )?;
                ids.push(id);
                by_name.entry(symbol.name.as_str()).or_insert(id);
            }

            for reference in &found.refs {
                // `impl Foo` is owned by `struct Foo` in this same file, falling back to the module node.
                let src = match &reference.from {
                    Owner::Symbol(index) => ids.get(*index).copied().unwrap_or(module_id),
                    Owner::Scope(name) => by_name.get(name.as_str()).copied().unwrap_or(module_id),
                    Owner::File => module_id,
                };
                let (dst, dst_name) = match &reference.to {
                    Target::Symbol(index) => (ids.get(*index).copied(), None),
                    Target::Name(name) => (None, Some(name.as_str())),
                };
                tx.execute(
                    "INSERT INTO refs (src, kind, dst, dst_name, line) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![src, reference.kind.as_str(), dst, dst_name, reference.line],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Forget a file entirely: its `files` row, its symbols, and its FTS entries.
    pub fn forget_files(&self, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        self.with(|conn| {
            let tx = conn.transaction()?;
            for path in paths {
                forget_symbols_of(&tx, path)?;
                tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Rebuild the whole edge table from `refs` — whole, not incremental, so a new target links immediately.
    /// Tiers are same file, same directory, same language, whole store; the first tier with a hit wins.
    /// Returns how many edges were written.
    pub fn rebuild_edges(&self) -> Result<usize> {
        self.with(|conn| {
            let tx = conn.transaction()?;

            let mut files: HashMap<i64, FileRow> = HashMap::new();
            {
                let mut stmt = tx.prepare("SELECT id, path, lang FROM files")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let id: i64 = row.get(0)?;
                    let path: String = row.get(1)?;
                    let dir = Path::new(&path)
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    files.insert(
                        id,
                        FileRow {
                            path,
                            dir,
                            lang: row.get(2)?,
                        },
                    );
                }
            }

            let mut by_name: HashMap<String, Vec<Candidate>> = HashMap::new();
            {
                let mut stmt = tx.prepare("SELECT id, name, kind, file_id FROM symbols")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let kind: String = row.get(2)?;
                    let name: String = row.get(1)?;
                    by_name.entry(name).or_default().push(Candidate {
                        id: row.get(0)?,
                        file: row.get(3)?,
                        module: kind == MODULE_KIND,
                    });
                }
            }

            tx.execute("DELETE FROM edges", [])?;
            let mut written = 0usize;
            {
                let mut insert = tx.prepare(
                    "INSERT OR IGNORE INTO edges (src, dst, kind, path, line) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                let mut stmt = tx.prepare(
                    "SELECT r.src, r.kind, r.dst, r.dst_name, r.line, s.file_id \
                     FROM refs r JOIN symbols s ON s.id = r.src",
                )?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let src: i64 = row.get(0)?;
                    let kind_text: String = row.get(1)?;
                    let Some(kind) = EdgeKind::parse(&kind_text) else {
                        continue;
                    };
                    let line: i64 = row.get(4)?;
                    let site: i64 = row.get(5)?;
                    let Some(file) = files.get(&site) else {
                        continue;
                    };

                    let exact: Option<i64> = row.get(2)?;
                    let targets: Vec<i64> = match exact {
                        Some(id) => vec![id],
                        None => {
                            let name: Option<String> = row.get(3)?;
                            let Some(name) = name else { continue };
                            match by_name.get(&name) {
                                Some(pool) => resolve(pool, kind, site, file, &files),
                                None => Vec::new(),
                            }
                        }
                    };

                    for dst in targets {
                        // A self-edge leads nowhere and turns every traversal into a loop to guard.
                        if dst == src {
                            continue;
                        }
                        written += insert.execute(params![
                            src,
                            dst,
                            kind.as_str(),
                            file.path.as_str(),
                            line
                        ])?;
                    }
                }
            }
            tx.commit()?;
            Ok(written)
        })
    }

    /// Record when the scan finished, epoch milliseconds.
    pub fn mark_scanned(&self, at: i64) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('scanned_at', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                params![at],
            )?;
            Ok(())
        })
    }

    /// Search symbols by name; the `LIKE` pass runs only when FTS5 finds nothing, since FTS5 cannot match mid-camelCase.
    pub fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<Symbol>> {
        let kind = kind.map(|k| k.as_str().to_string());
        let limit = limit as i64;
        self.with(|conn| {
            if let Some(expression) = fts_expression(query) {
                let sql = format!(
                    "{SELECT_SYMBOL} JOIN symbols_fts ON symbols_fts.rowid = s.id \
                     WHERE symbols_fts MATCH ?1 AND (?2 IS NULL OR s.kind = ?2) AND {NOT_MODULE} \
                     ORDER BY bm25(symbols_fts, 10.0, 3.0, 1.0), s.name LIMIT ?3"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![expression, kind, limit], read_symbol)?;
                let hits = rows.collect::<rusqlite::Result<Vec<Symbol>>>()?;
                if !hits.is_empty() {
                    return Ok(hits);
                }
            }
            let sql = format!(
                "{SELECT_SYMBOL} WHERE s.name LIKE ?1 ESCAPE '\\' \
                 AND (?2 IS NULL OR s.kind = ?2) AND {NOT_MODULE} \
                 ORDER BY length(s.name), s.name LIMIT ?3"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![like_pattern(query), kind, limit], read_symbol)?;
            Ok(rows.collect::<rusqlite::Result<Vec<Symbol>>>()?)
        })
    }

    /// A file's symbol map, in source order.
    pub fn outline(&self, path: &str) -> Result<Vec<Symbol>> {
        self.with(|conn| {
            let sql = format!(
                "{SELECT_SYMBOL} WHERE f.path = ?1 AND {NOT_MODULE} \
                 ORDER BY s.start_line, s.end_line DESC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![path], read_symbol)?;
            Ok(rows.collect::<rusqlite::Result<Vec<Symbol>>>()?)
        })
    }

    /// Whether the file is indexed, telling "no symbols" apart from "never scanned".
    pub fn knows(&self, path: &str) -> Result<bool> {
        self.with(|conn| {
            let found: Option<i64> = conn
                .query_row("SELECT id FROM files WHERE path = ?1", params![path], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(found.is_some())
        })
    }

    pub fn symbol_count(&self) -> Result<i64> {
        self.with(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM symbols s WHERE s.kind <> 'module'",
                [],
                |r| r.get(0),
            )?)
        })
    }

    pub fn edge_count(&self) -> Result<i64> {
        self.with(|conn| Ok(conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0))?))
    }

    /// Nodes with exactly this name; `Foo::bar` is split, because the model copies back the qualified name it was shown.
    pub fn nodes_named(&self, name: &str) -> Result<Vec<GraphNode>> {
        let (parent, leaf) = match name.rsplit_once("::") {
            Some((parent, leaf)) => (Some(parent.to_string()), leaf.to_string()),
            None => (None, name.to_string()),
        };
        self.with(|conn| {
            let sql = format!(
                "{SELECT_NODE} WHERE s.name = ?1 AND (?2 IS NULL OR s.parent = ?2) \
                 AND {NOT_MODULE} ORDER BY f.path, s.start_line"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![leaf, parent], read_node)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphNode>>>()?)
        })
    }

    pub fn nodes_by_ids(&self, ids: &[i64]) -> Result<Vec<GraphNode>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let sql = format!("{SELECT_NODE} WHERE s.id IN ({})", placeholders(ids.len()));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids), read_node)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphNode>>>()?)
        })
    }

    /// Every edge touching one of these nodes, both directions; the reverse leg needs the `dst` index.
    pub fn edges_touching(&self, ids: &[i64]) -> Result<Vec<GraphEdge>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let holes = placeholders(ids.len());
            let sql = format!(
                "SELECT src, dst, kind FROM edges WHERE src IN ({holes}) \
                 UNION SELECT src, dst, kind FROM edges WHERE dst IN ({holes})"
            );
            // Both `UNION` halves reuse `?1..?n`, so the parameters are bound once: SQLite counts distinct holes.
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids), read_edge)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphEdge>>>()?)
        })
    }

    /// Neighbours along one edge kind and direction; `forward` follows the arrow.
    pub fn step(&self, ids: &[i64], kind: EdgeKind, forward: bool) -> Result<Vec<GraphEdge>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|conn| {
            let column = if forward { "src" } else { "dst" };
            let sql = format!(
                "SELECT src, dst, kind FROM edges WHERE kind = ?1 AND {column} IN ({})",
                placeholders_from(ids.len(), 2)
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut args: Vec<rusqlite::types::Value> =
                vec![rusqlite::types::Value::Text(kind.as_str().to_string())];
            args.extend(ids.iter().map(|id| rusqlite::types::Value::from(*id)));
            let rows = stmt.query_map(params_from_iter(args), read_edge)?;
            Ok(rows.collect::<rusqlite::Result<Vec<GraphEdge>>>()?)
        })
    }

    /// Edges observed in one file with both ends attached; for tests and debugging.
    pub fn edges_of_file(&self, path: &str) -> Result<Vec<(GraphNode, EdgeKind, GraphNode)>> {
        self.with(|conn| {
            let sql = "SELECT e.kind, \
                 a.id, a.name, a.kind, af.path, a.start_line, \
                 b.id, b.name, b.kind, bf.path, b.start_line \
                 FROM edges e \
                 JOIN symbols a ON a.id = e.src JOIN files af ON af.id = a.file_id \
                 JOIN symbols b ON b.id = e.dst JOIN files bf ON bf.id = b.file_id \
                 WHERE e.path = ?1 ORDER BY e.line, e.kind, b.name";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![path], |row| {
                let kind: String = row.get(0)?;
                Ok((
                    GraphNode {
                        id: row.get(1)?,
                        name: row.get(2)?,
                        kind: row.get(3)?,
                        path: row.get(4)?,
                        line: row.get(5)?,
                    },
                    EdgeKind::parse(&kind).unwrap_or(EdgeKind::References),
                    GraphNode {
                        id: row.get(6)?,
                        name: row.get(7)?,
                        kind: row.get(8)?,
                        path: row.get(9)?,
                        line: row.get(10)?,
                    },
                ))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn stats(&self) -> Result<Stats> {
        self.with(|conn| {
            let scanned_at: Option<i64> = conn
                .query_row("SELECT value FROM meta WHERE key = 'scanned_at'", [], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(Stats {
                files: conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?,
                symbols: conn.query_row(
                    "SELECT count(*) FROM symbols s WHERE s.kind <> 'module'",
                    [],
                    |r| r.get(0),
                )?,
                edges: conn.query_row("SELECT count(*) FROM edges", [], |r| r.get(0))?,
                languages: languages(conn)?,
                scanned_at,
            })
        })
    }

    /// The architecture map; `directories` is capped at `dir_cap`, `central` at `central_cap`.
    pub fn overview(&self, dir_cap: usize, central_cap: usize) -> Result<Overview> {
        self.with(|conn| {
            let mut folders: HashMap<String, DirectorySummary> = HashMap::new();
            {
                let mut stmt = conn.prepare(
                    "SELECT f.path, (SELECT count(*) FROM symbols s \
                      WHERE s.file_id = f.id AND s.kind <> 'module') FROM files f",
                )?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let path: String = row.get(0)?;
                    let symbols: u32 = row.get(1)?;
                    let dir = Path::new(&path)
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    let entry = folders.entry(dir.clone()).or_insert(DirectorySummary {
                        path: dir,
                        files: 0,
                        symbols: 0,
                    });
                    entry.files += 1;
                    entry.symbols += symbols;
                }
            }
            let mut directories: Vec<DirectorySummary> = folders.into_values().collect();
            // Most symbols first: an unfamiliar repo is read from its busiest place, not alphabetically.
            directories.sort_by(|a, b| b.symbols.cmp(&a.symbols).then_with(|| a.path.cmp(&b.path)));
            let directories_omitted = directories.len().saturating_sub(dir_cap) as u32;
            directories.truncate(dir_cap);

            // `contains` is excluded from degree, or the ranking just reports which file is longest.
            let mut stmt = conn.prepare(
                "SELECT s.id, s.name, s.kind, f.path, s.start_line, d.incoming, d.outgoing \
                 FROM (SELECT id, sum(inc) AS incoming, sum(outg) AS outgoing FROM ( \
                         SELECT dst AS id, 1 AS inc, 0 AS outg FROM edges WHERE kind <> 'contains' \
                         UNION ALL \
                         SELECT src AS id, 0 AS inc, 1 AS outg FROM edges WHERE kind <> 'contains' \
                       ) GROUP BY id) d \
                 JOIN symbols s ON s.id = d.id JOIN files f ON f.id = s.file_id \
                 WHERE s.kind <> 'module' \
                 ORDER BY (d.incoming + d.outgoing) DESC, s.name LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![central_cap as i64], |row| {
                Ok(CentralSymbol {
                    node: GraphNode {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        kind: row.get(2)?,
                        path: row.get(3)?,
                        line: row.get(4)?,
                    },
                    incoming: row.get(5)?,
                    outgoing: row.get(6)?,
                })
            })?;
            let central = rows.collect::<rusqlite::Result<Vec<CentralSymbol>>>()?;

            Ok(Overview {
                directories,
                languages: languages(conn)?,
                central,
                directories_omitted,
            })
        })
    }
}

/// A file with its directory pre-split, to avoid re-slicing the string per reference.
struct FileRow {
    path: String,
    dir: String,
    lang: String,
}

#[derive(Clone, Copy)]
struct Candidate {
    id: i64,
    file: i64,
    module: bool,
}

/// Four tiers; only the first tier with a hit is used — see [`Store::rebuild_edges`].
fn resolve(
    pool: &[Candidate],
    kind: EdgeKind,
    site: i64,
    file: &FileRow,
    files: &HashMap<i64, FileRow>,
) -> Vec<i64> {
    let allowed: Vec<&Candidate> = pool
        .iter()
        .filter(|candidate| kind.may_target_module() || !candidate.module)
        .collect();
    if allowed.is_empty() {
        return Vec::new();
    }
    let tiers: [&dyn Fn(&Candidate) -> bool; 4] = [
        &|candidate: &Candidate| candidate.file == site,
        &|candidate: &Candidate| {
            files
                .get(&candidate.file)
                .is_some_and(|f| f.dir == file.dir)
        },
        &|candidate: &Candidate| {
            files
                .get(&candidate.file)
                .is_some_and(|f| f.lang == file.lang)
        },
        &|_: &Candidate| true,
    ];
    for tier in tiers {
        let hits: Vec<i64> = allowed
            .iter()
            .filter(|candidate| tier(candidate))
            .map(|candidate| candidate.id)
            .collect();
        if hits.is_empty() {
            continue;
        }
        return if hits.len() > MAX_CANDIDATES {
            Vec::new()
        } else {
            hits
        };
    }
    Vec::new()
}

/// The module node's name: the file stem, which is also what a `use crate::store::...` looks up.
fn module_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn languages(conn: &Connection) -> rusqlite::Result<Vec<(String, u32)>> {
    let mut stmt =
        conn.prepare("SELECT lang, count(*) AS n FROM files GROUP BY lang ORDER BY n DESC, lang")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

fn placeholders(count: usize) -> String {
    placeholders_from(count, 1)
}

fn placeholders_from(count: usize, first: usize) -> String {
    (first..first + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn forget_symbols_of(tx: &rusqlite::Transaction<'_>, path: &str) -> Result<()> {
    // FTS first: once the `symbols` rows are gone, nothing says which FTS rowids to delete.
    tx.execute(
        "DELETE FROM symbols_fts WHERE rowid IN \
         (SELECT s.id FROM symbols s JOIN files f ON f.id = s.file_id WHERE f.path = ?1)",
        params![path],
    )?;
    // `refs` and `edges` follow by `ON DELETE CASCADE`, inbound edges too; the next resolve rebuilds the valid ones.
    tx.execute(
        "DELETE FROM symbols WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
        params![path],
    )?;
    Ok(())
}

fn read_symbol(row: &Row<'_>) -> rusqlite::Result<Symbol> {
    let kind: String = row.get(1)?;
    Ok(Symbol {
        name: row.get(0)?,
        // An unknown label can only come from an older build of this crate; `type` reads better than failing.
        kind: SymbolKind::parse(&kind).unwrap_or(SymbolKind::Type),
        parent: row.get(2)?,
        start_line: row.get(3)?,
        end_line: row.get(4)?,
        signature: row.get(5)?,
        path: row.get(6)?,
    })
}

fn read_node(row: &Row<'_>) -> rusqlite::Result<GraphNode> {
    Ok(GraphNode {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        path: row.get(3)?,
        line: row.get(4)?,
    })
}

fn read_edge(row: &Row<'_>) -> rusqlite::Result<GraphEdge> {
    let kind: String = row.get(2)?;
    Ok(GraphEdge {
        src: row.get(0)?,
        dst: row.get(1)?,
        kind: EdgeKind::parse(&kind).unwrap_or(EdgeKind::References),
    })
}

/// Turn a user query into a safe FTS5 expression: never concatenated into MATCH syntax, but tokenised and quoted.
fn fts_expression(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\"*"))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" AND "))
    }
}

/// `%` and `_` are `LIKE` wildcards; escaping them makes `foo_bar` match `foo_bar`.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        tracing::debug!(mode, "could not enable WAL for this index store");
    }
    // The index is rebuildable, so a transaction lost to a power cut costs one re-parse, not data.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn ensure_schema(conn: &mut Connection) -> Result<()> {
    let app_id: i32 = conn.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let populated: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'files'",
        [],
        |row| row.get(0),
    )?;

    if populated > 0 {
        if app_id != APPLICATION_ID {
            return Err(IndexError::Store(
                "tệp này không phải kho chỉ mục; từ chối ghi đè".into(),
            ));
        }
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        tracing::info!(
            from = version,
            to = SCHEMA_VERSION,
            "index schema is out of date, rebuilding from scratch"
        );
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS meta; \
             DROP TABLE IF EXISTS edges; \
             DROP TABLE IF EXISTS refs; \
             DROP TABLE IF EXISTS symbols_fts; \
             DROP TABLE IF EXISTS symbols; \
             DROP TABLE IF EXISTS files;",
        )?;
        tx.commit()?;
    }

    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.commit()?;
    conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
