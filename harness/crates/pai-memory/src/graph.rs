//! The knowledge graph itself: schema, migration, and every SQL statement.
//!
//! This module deliberately knows nothing about `pai-tools`. A fact's shape must not depend on
//! how a model happens to ask for it, and keeping the line here means the graph can be tested —
//! and later read by the UI — without building a tool call. `pai-rag-core` is split along the
//! same line for the same reason; here one module is enough, since there is no ranking or
//! chunking to keep company.
//!
//! Why SQLite at all: the MCP server this replaces keeps the graph in one JSONL file and reloads
//! the whole thing on every call. That is fine for fifty facts and hopeless for fifty thousand.
//! Every query below is indexed, and every one that can return an unbounded set takes a `limit`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Bumped only when the tables below change shape; recorded in `meta` so a future migration can
/// tell an old file from a new one without sniffing the schema.
pub const SCHEMA_VERSION: u32 = 1;
/// How many observation rows to rank per entity asked for. One entity can own hundreds of
/// matching observations, so ranking exactly `limit` rows would return one entity and call it a
/// search; eight per slot is enough headroom without reading the table.
const FTS_FANOUT: usize = 8;
/// Absolute ceiling on ranked rows, so a large `limit` cannot turn into a scan.
const MAX_FTS_ROWS: usize = 500;
pub const META_SCHEMA: &str = "schema.version";

/// Every statement is `IF NOT EXISTS`, so `initialize` is the migration: running it on an open
/// database is a no-op, which is what makes reopening the same file safe.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS entities (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  kind       TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Recency is the default order of a truncated read, so it must not be a table scan + sort.
-- Ties are broken by `id` at the query, not here: two facts written in the same millisecond are
-- common, and falling back to name order would put the oldest entity at the top of a "recent" read.
CREATE INDEX IF NOT EXISTS entities_by_updated ON entities (updated_at DESC);

CREATE TABLE IF NOT EXISTS observations (
  id         INTEGER PRIMARY KEY,
  entity_id  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  body       TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

-- The graph is written by a model that will happily state the same fact twice; the uniqueness
-- lives in the index so `INSERT OR IGNORE` can do the deduplication without a read first.
CREATE UNIQUE INDEX IF NOT EXISTS observations_unique ON observations (entity_id, body);

CREATE TABLE IF NOT EXISTS relations (
  id         INTEGER PRIMARY KEY,
  from_id    INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  to_id      INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  verb       TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS relations_unique ON relations (from_id, verb, to_id);
-- The forward direction is covered by the unique index above; this one answers "who points at X".
CREATE INDEX IF NOT EXISTS relations_by_to ON relations (to_id);

-- External-content FTS over observations only. Entity names are matched with LIKE instead:
-- a name is short and usually looked up whole, and an FTS token match on a name would rank
-- "Vinh" and "Vinh Pham" identically.
CREATE VIRTUAL TABLE IF NOT EXISTS observations_fts USING fts5(
  body, content = 'observations', content_rowid = 'id',
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS observations_ai AFTER INSERT ON observations BEGIN
  INSERT INTO observations_fts (rowid, body) VALUES (new.id, new.body);
END;

CREATE TRIGGER IF NOT EXISTS observations_ad AFTER DELETE ON observations BEGIN
  INSERT INTO observations_fts (observations_fts, rowid, body) VALUES ('delete', old.id, old.body);
END;

CREATE TRIGGER IF NOT EXISTS observations_au AFTER UPDATE ON observations BEGIN
  INSERT INTO observations_fts (observations_fts, rowid, body) VALUES ('delete', old.id, old.body);
  INSERT INTO observations_fts (rowid, body) VALUES (new.id, new.body);
END;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("lỗi SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("không tạo được thư mục chứa tệp trí nhớ: {0}")]
    Io(#[from] std::io::Error),
}

pub type GraphResult<T> = Result<T, GraphError>;

/// One node. `name` is the identity — the same choice the MCP server made, and the reason a
/// model can talk about "Vinh" across sessions without carrying an id around.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Capped by the caller's `per_entity`; `observations_total` says how many there really are.
    pub observations: Vec<String>,
    pub observations_total: i64,
    pub updated_at: i64,
}

/// One directed edge, rendered with names rather than ids because names are what the model reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub from: String,
    pub verb: String,
    pub to: String,
}

/// What a `remember` call wants written. Entities and relations arrive together so that an edge
/// and the nodes it needs can be declared in one round trip.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityInput {
    pub name: String,
    pub kind: String,
    pub observations: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationInput {
    pub from: String,
    pub verb: String,
    pub to: String,
}

/// What one entity's observations should be forgotten.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationTarget {
    pub entity: String,
    pub observations: Vec<String>,
}

/// The receipt of a write. Counts rather than a bare "ok", so the model can tell "already knew
/// that" from "wrote that down" without reading the graph back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub entities_created: usize,
    pub entities_updated: usize,
    pub observations_added: usize,
    pub relations_created: usize,
    /// Relations skipped because an endpoint does not exist. Reported instead of auto-creating a
    /// typeless node: a graph full of placeholder entities is worse than a clear refusal.
    pub skipped_relations: Vec<RelationInput>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forgotten {
    pub entities: usize,
    pub observations: usize,
    pub relations: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub entities: i64,
    pub observations: i64,
    pub relations: i64,
}

/// Why an entity came back from [`Graph::search`]; shown to the model so it can judge a weak hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Name,
    Observation,
    Both,
}

impl MatchedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchedBy::Name => "tên",
            MatchedBy::Observation => "quan sát",
            MatchedBy::Both => "tên + quan sát",
        }
    }
}

/// A hit: the entity plus why it matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub entity: Entity,
    pub matched_by: MatchedBy,
}

/// The graph, over one SQLite file.
///
/// Not internally synchronised, exactly like [`pai_rag_core::store::Store`]: a write is a
/// transaction and needs `&mut self`, so the lock belongs to whoever shares the handle. The
/// plugin wraps it in a `parking_lot::Mutex` and hands out `Arc`s of that.
pub struct Graph {
    path: Option<PathBuf>,
    connection: Connection,
}

impl Drop for Graph {
    fn drop(&mut self) {
        // Leave no WAL behind for the next process to replay: the app is a desktop app and is
        // killed abruptly often enough that a clean file on exit is worth one pragma.
        let _ = self
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
    }
}

impl Graph {
    pub fn open(path: impl AsRef<Path>) -> GraphResult<Graph> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        initialize(&connection)?;
        Ok(Graph {
            path: Some(path.to_owned()),
            connection,
        })
    }

    /// For tests, and for the case where no writable location exists — a memory that forgets on
    /// exit still beats a tool that fails on every call.
    pub fn in_memory() -> GraphResult<Graph> {
        let connection = Connection::open_in_memory()?;
        initialize(&connection)?;
        Ok(Graph {
            path: None,
            connection,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn stats(&self) -> GraphResult<Stats> {
        let count = |table: &str| -> Result<i64, rusqlite::Error> {
            self.connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
        };
        Ok(Stats {
            entities: count("entities")?,
            observations: count("observations")?,
            relations: count("relations")?,
        })
    }

    /// Write entities, their observations, and edges, in one transaction.
    ///
    /// Idempotent on purpose: the model is expected to re-state facts it already knows, and a
    /// second `remember` of the same sentence must not double it. Everything is upsert or
    /// `INSERT OR IGNORE`, so a retry after a dropped result is safe.
    pub fn remember(
        &mut self,
        entities: &[EntityInput],
        relations: &[RelationInput],
    ) -> GraphResult<Written> {
        let now = now_millis();
        let mut report = Written::default();
        let tx = self.connection.transaction()?;

        for entity in entities {
            let name = entity.name.trim();
            if name.is_empty() {
                continue;
            }
            let existing: Option<(i64, String)> = tx
                .query_row(
                    "SELECT id, kind FROM entities WHERE name = ?",
                    [name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;

            let kind = entity.kind.trim();
            let id = match existing {
                Some((id, old_kind)) => {
                    // An empty `kind` on an update means "leave it alone", so a call that only
                    // adds an observation does not have to repeat the type it does not know.
                    if !kind.is_empty() && kind != old_kind {
                        tx.execute(
                            "UPDATE entities SET kind = ?, updated_at = ? WHERE id = ?",
                            params![kind, now, id],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE entities SET updated_at = ? WHERE id = ?",
                            params![now, id],
                        )?;
                    }
                    report.entities_updated += 1;
                    id
                }
                None => {
                    tx.execute(
                        "INSERT INTO entities (name, kind, created_at, updated_at) VALUES (?, ?, ?, ?)",
                        params![name, kind, now, now],
                    )?;
                    report.entities_created += 1;
                    tx.last_insert_rowid()
                }
            };

            for body in &entity.observations {
                let body = body.trim();
                if body.is_empty() {
                    continue;
                }
                // `OR IGNORE` leans on `observations_unique`; the affected-row count is the
                // honest answer to "was this new?".
                report.observations_added += tx.execute(
                    "INSERT OR IGNORE INTO observations (entity_id, body, created_at) VALUES (?, ?, ?)",
                    params![id, body, now],
                )?;
            }
        }

        for relation in relations {
            let (from, verb, to) = (
                relation.from.trim(),
                relation.verb.trim(),
                relation.to.trim(),
            );
            if from.is_empty() || verb.is_empty() || to.is_empty() {
                report.skipped_relations.push(relation.clone());
                continue;
            }
            let ends: Option<(i64, i64)> = tx
                .query_row(
                    "SELECT (SELECT id FROM entities WHERE name = ?1), \
                            (SELECT id FROM entities WHERE name = ?2)",
                    params![from, to],
                    |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .optional()?
                .and_then(|(a, b)| Some((a?, b?)));

            let Some((from_id, to_id)) = ends else {
                report.skipped_relations.push(relation.clone());
                continue;
            };
            report.relations_created += tx.execute(
                "INSERT OR IGNORE INTO relations (from_id, to_id, verb, created_at) VALUES (?, ?, ?, ?)",
                params![from_id, to_id, verb, now],
            )?;
        }

        tx.commit()?;
        Ok(report)
    }

    /// Delete entities by name, the observations they own, and the edges they touch.
    ///
    /// The cascade is the schema's job (`ON DELETE CASCADE` plus `foreign_keys = ON`), not this
    /// function's; doing it by hand is how half-deleted edges appear.
    pub fn forget(
        &mut self,
        entities: &[String],
        observations: &[ObservationTarget],
        relations: &[RelationInput],
    ) -> GraphResult<Forgotten> {
        let mut report = Forgotten::default();
        let tx = self.connection.transaction()?;

        for target in observations {
            let entity = target.entity.trim();
            if entity.is_empty() || target.observations.is_empty() {
                continue;
            }
            let mut statement = tx.prepare(
                "DELETE FROM observations WHERE body = ?2 AND entity_id = \
                 (SELECT id FROM entities WHERE name = ?1)",
            )?;
            for body in &target.observations {
                report.observations += statement.execute(params![entity, body.trim()])?;
            }
        }

        for relation in relations {
            report.relations += tx.execute(
                "DELETE FROM relations WHERE verb = ?3 \
                 AND from_id = (SELECT id FROM entities WHERE name = ?1) \
                 AND to_id   = (SELECT id FROM entities WHERE name = ?2)",
                params![
                    relation.from.trim(),
                    relation.to.trim(),
                    relation.verb.trim()
                ],
            )?;
        }

        for name in entities {
            report.entities += tx.execute("DELETE FROM entities WHERE name = ?", [name.trim()])?;
        }

        tx.commit()?;
        Ok(report)
    }

    /// Read entities by exact name, or the whole graph most-recently-touched first.
    ///
    /// `limit` is not optional and has no "unlimited" spelling: a read with no ceiling is how a
    /// long-lived graph eats a context window.
    pub fn entities(
        &self,
        names: Option<&[String]>,
        limit: usize,
        per_entity: usize,
    ) -> GraphResult<Vec<Entity>> {
        let rows: Vec<(i64, String, String, i64)> = match names {
            Some(names) if !names.is_empty() => {
                let holes = vec!["?"; names.len()].join(", ");
                let sql = format!(
                    "SELECT id, name, kind, updated_at FROM entities WHERE name IN ({holes}) \
                     ORDER BY updated_at DESC, id DESC LIMIT ?"
                );
                let mut statement = self.connection.prepare(&sql)?;
                // Typed bindings, not a `Vec<String>`: SQLite refuses a text value in `LIMIT`,
                // so the last parameter has to stay an integer.
                let mut bindings: Vec<rusqlite::types::Value> = names
                    .iter()
                    .map(|name| rusqlite::types::Value::Text(name.trim().to_string()))
                    .collect();
                bindings.push(rusqlite::types::Value::Integer(limit as i64));
                let rows = statement.query_map(params_from_iter(bindings), entity_head)?;
                rows.collect::<Result<_, _>>()?
            }
            // An empty name list means "no filter", not "no results": `read` with no argument is
            // the whole-graph read, and the only difference is the WHERE clause.
            _ => {
                let mut statement = self.connection.prepare(
                    "SELECT id, name, kind, updated_at FROM entities \
                     ORDER BY updated_at DESC, id DESC LIMIT ?",
                )?;
                let rows = statement.query_map([limit as i64], entity_head)?;
                rows.collect::<Result<_, _>>()?
            }
        };
        self.hydrate(rows, per_entity)
    }

    /// Find entities whose name or type contains `query`, or that own an observation matching it.
    ///
    /// Two indexes, one merge: name hits come first because an exact name is the strongest signal
    /// a graph keyed by name can give, then FTS hits ordered by bm25.
    pub fn search(&self, query: &str, limit: usize, per_entity: usize) -> GraphResult<Vec<Hit>> {
        let needle = query.trim();
        if needle.is_empty() {
            return Ok(Vec::new());
        }

        let lowered = needle.to_lowercase();
        let pattern = like_pattern(&lowered);
        // `lower()` in SQLite folds ASCII only, so a Vietnamese name typed in a different case
        // will not fold; matching the raw needle as well covers the common "typed it as written"
        // case without dragging in an ICU build.
        let raw_pattern = like_pattern(needle);
        let mut by_name: Vec<i64> = {
            let mut statement = self.connection.prepare(
                "SELECT id FROM entities \
                 WHERE lower(name) LIKE ?1 ESCAPE '\\' OR name LIKE ?2 ESCAPE '\\' \
                    OR lower(kind) LIKE ?1 ESCAPE '\\' \
                 ORDER BY (lower(name) = ?3) DESC, length(name) ASC, updated_at DESC LIMIT ?4",
            )?;
            let rows = statement.query_map(
                params![pattern, raw_pattern, lowered, limit as i64],
                |row| row.get(0),
            )?;
            rows.collect::<Result<_, _>>()?
        };

        let by_observation: Vec<i64> = match fts_expressions(needle) {
            None => Vec::new(),
            Some((strict, loose)) => {
                // bm25() only resolves in a query that reads the FTS table and nothing else — a
                // join in the same SELECT makes SQLite refuse the auxiliary function. So rank
                // observation rowids here, and map them to entities in a second step.
                let sql = "SELECT rowid FROM observations_fts WHERE observations_fts MATCH ?1 \
                           ORDER BY bm25(observations_fts) LIMIT ?2";
                let fetch = limit.saturating_mul(FTS_FANOUT).min(MAX_FTS_ROWS) as i64;
                let run = |expression: &str| -> Result<Vec<i64>, rusqlite::Error> {
                    let mut statement = self.connection.prepare(sql)?;
                    statement
                        .query_map(params![expression, fetch], |row| row.get(0))?
                        .collect()
                };
                // All terms first; fall back to any term, so a three-word question still answers.
                let mut ranked = run(&strict)?;
                if ranked.is_empty() {
                    ranked = run(&loose)?;
                }
                self.owners_of(&ranked, limit)?
            }
        };

        let named: std::collections::HashSet<i64> = by_name.iter().copied().collect();
        let observed: std::collections::HashSet<i64> = by_observation.iter().copied().collect();
        for id in by_observation {
            if !named.contains(&id) {
                by_name.push(id);
            }
        }
        by_name.truncate(limit);

        let heads = self.heads_by_id(&by_name)?;
        let ordered: Vec<(i64, String, String, i64)> = by_name
            .iter()
            .filter_map(|id| heads.get(id).cloned())
            .collect();

        let entities = self.hydrate(ordered, per_entity)?;
        Ok(entities
            .into_iter()
            .map(|entity| {
                let matched_by = match (named.contains(&entity.id), observed.contains(&entity.id)) {
                    (true, true) => MatchedBy::Both,
                    (true, false) => MatchedBy::Name,
                    _ => MatchedBy::Observation,
                };
                Hit { entity, matched_by }
            })
            .collect())
    }

    /// Edges with both ends inside `ids`.
    ///
    /// Both ends on purpose: an edge to an entity the model was not shown is a dangling reference
    /// it will then ask about, which costs a round trip to learn nothing.
    pub fn relations_among(&self, ids: &[i64]) -> GraphResult<Vec<Relation>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let holes = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT a.name, r.verb, b.name FROM relations r \
             JOIN entities a ON a.id = r.from_id \
             JOIN entities b ON b.id = r.to_id \
             WHERE r.from_id IN ({holes}) AND r.to_id IN ({holes}) \
             ORDER BY a.name, r.verb, b.name"
        );
        let mut statement = self.connection.prepare(&sql)?;
        let bindings: Vec<i64> = ids.iter().chain(ids.iter()).copied().collect();
        let rows = statement.query_map(params_from_iter(bindings), |row| {
            Ok(Relation {
                from: row.get(0)?,
                verb: row.get(1)?,
                to: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Ranked observation rowids to their entities, deduplicated, best rank first.
    ///
    /// Separate from the FTS query for the reason given at the call site; the fanout there is
    /// what keeps one talkative entity from filling the whole page with its own observations.
    fn owners_of(&self, observation_ids: &[i64], limit: usize) -> GraphResult<Vec<i64>> {
        if observation_ids.is_empty() {
            return Ok(Vec::new());
        }
        let holes = vec!["?"; observation_ids.len()].join(", ");
        let sql = format!("SELECT id, entity_id FROM observations WHERE id IN ({holes})");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(observation_ids.iter().copied()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?;
        let owner: HashMap<i64, i64> = rows.collect::<Result<_, _>>()?;

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for id in observation_ids {
            let Some(entity_id) = owner.get(id) else {
                continue;
            };
            if seen.insert(*entity_id) {
                out.push(*entity_id);
                if out.len() == limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn heads_by_id(&self, ids: &[i64]) -> GraphResult<HashMap<i64, (i64, String, String, i64)>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let holes = vec!["?"; ids.len()].join(", ");
        let sql = format!("SELECT id, name, kind, updated_at FROM entities WHERE id IN ({holes})");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(ids.iter().copied()), entity_head)?;
        let mut map = HashMap::new();
        for row in rows {
            let row = row?;
            map.insert(row.0, row);
        }
        Ok(map)
    }

    /// Attach each entity's most recent `per_entity` observations plus the true total.
    ///
    /// One query per entity: the caller has already bounded the set to at most a few dozen rows,
    /// and a local SQLite point lookup is cheaper than the window function that would avoid it.
    ///
    /// `id DESC` in the query and a reverse in Rust, not `id ASC`: taking the *oldest* rows would
    /// mean that once an entity owns more than `per_entity` observations, a fact learned today can
    /// never be read back — the worst failure a memory can have. Reversing afterwards keeps the
    /// lines in the order they were written, which is how they read.
    fn hydrate(
        &self,
        heads: Vec<(i64, String, String, i64)>,
        per_entity: usize,
    ) -> GraphResult<Vec<Entity>> {
        let mut bodies = self.connection.prepare(
            "SELECT body FROM observations WHERE entity_id = ? ORDER BY id DESC LIMIT ?",
        )?;
        let mut totals = self
            .connection
            .prepare("SELECT COUNT(*) FROM observations WHERE entity_id = ?")?;

        let mut out = Vec::with_capacity(heads.len());
        for (id, name, kind, updated_at) in heads {
            let mut observations: Vec<String> = bodies
                .query_map(params![id, per_entity as i64], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            observations.reverse();
            let observations_total: i64 = totals.query_row([id], |row| row.get(0))?;
            out.push(Entity {
                id,
                name,
                kind,
                observations,
                observations_total,
                updated_at,
            });
        }
        Ok(out)
    }
}

fn entity_head(row: &rusqlite::Row<'_>) -> Result<(i64, String, String, i64), rusqlite::Error> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

/// Pragmas then schema, in that order: `foreign_keys` is per-connection, so a cascade only works
/// if every connection turns it on.
fn initialize(connection: &Connection) -> GraphResult<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.execute_batch(SCHEMA)?;
    connection.execute(
        "INSERT INTO meta (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![META_SCHEMA, SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

/// Milliseconds since the epoch. `chrono` rather than `SystemTime`, so a clock behind the epoch
/// is a negative number instead of an error path nobody would handle usefully.
fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `%` and `_` inside a user's words are wildcards to SQLite; escaped here so searching for
/// "100%" does not match everything.
fn like_pattern(needle: &str) -> String {
    let escaped = needle
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// A strict (all terms) and a loose (any term) FTS expression. Terms are quoted, which is what
/// keeps an apostrophe or a `-` in the user's question from being read as FTS syntax.
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

    fn entity(name: &str, kind: &str, observations: &[&str]) -> EntityInput {
        EntityInput {
            name: name.to_string(),
            kind: kind.to_string(),
            observations: observations.iter().map(|body| body.to_string()).collect(),
        }
    }

    fn edge(from: &str, verb: &str, to: &str) -> RelationInput {
        RelationInput {
            from: from.to_string(),
            verb: verb.to_string(),
            to: to.to_string(),
        }
    }

    fn seeded() -> GraphResult<Graph> {
        let mut graph = Graph::in_memory()?;
        graph.remember(
            &[
                entity(
                    "Vinh",
                    "người",
                    &["Thích trả lời ngắn gọn", "Làm việc bằng tiếng Việt"],
                ),
                entity("Private AI", "dự án", &["Ứng dụng desktop viết bằng Rust"]),
            ],
            &[edge("Vinh", "phát triển", "Private AI")],
        )?;
        Ok(graph)
    }

    #[test]
    fn migration_runs_twice_without_damage() -> GraphResult<()> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("memory.sqlite3");

        {
            let mut graph = Graph::open(&path)?;
            graph.remember(&[entity("Vinh", "người", &["Sự thật cũ"])], &[])?;
        }
        // Reopening runs `initialize` again over a populated file; nothing may be lost, and no
        // `CREATE` may fail.
        let graph = Graph::open(&path)?;
        assert_eq!(graph.stats()?.entities, 1);
        assert_eq!(graph.stats()?.observations, 1);
        Ok(())
    }

    #[test]
    fn remember_then_read_back() -> GraphResult<()> {
        let graph = seeded()?;
        let rows = graph.entities(Some(&["Vinh".to_string()]), 10, 10)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "người");
        assert_eq!(rows[0].observations_total, 2);

        let ids: Vec<i64> = graph.entities(None, 10, 10)?.iter().map(|e| e.id).collect();
        let edges = graph.relations_among(&ids)?;
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].verb, "phát triển");
        Ok(())
    }

    #[test]
    fn remembering_the_same_fact_twice_changes_nothing() -> GraphResult<()> {
        let mut graph = seeded()?;
        let again = graph.remember(
            &[entity("Vinh", "người", &["Thích trả lời ngắn gọn"])],
            &[edge("Vinh", "phát triển", "Private AI")],
        )?;
        assert_eq!(again.entities_created, 0);
        assert_eq!(again.observations_added, 0);
        assert_eq!(again.relations_created, 0);
        assert_eq!(graph.stats()?.observations, 3);
        Ok(())
    }

    #[test]
    fn empty_kind_on_update_keeps_the_old_one() -> GraphResult<()> {
        let mut graph = seeded()?;
        graph.remember(&[entity("Vinh", "", &["Sống ở Hà Nội"])], &[])?;
        let rows = graph.entities(Some(&["Vinh".to_string()]), 10, 10)?;
        assert_eq!(rows[0].kind, "người");
        assert_eq!(rows[0].observations_total, 3);
        Ok(())
    }

    #[test]
    fn a_relation_with_a_missing_end_is_reported_not_invented() -> GraphResult<()> {
        let mut graph = seeded()?;
        let report = graph.remember(&[], &[edge("Vinh", "quen", "Ai Đó")])?;
        assert_eq!(report.relations_created, 0);
        assert_eq!(report.skipped_relations.len(), 1);
        // The missing endpoint must not have been created as a side effect.
        assert!(
            graph
                .entities(Some(&["Ai Đó".to_string()]), 10, 10)?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn deleting_an_entity_takes_its_observations_and_edges() -> GraphResult<()> {
        let mut graph = seeded()?;
        let report = graph.forget(&["Vinh".to_string()], &[], &[])?;
        assert_eq!(report.entities, 1);

        let stats = graph.stats()?;
        assert_eq!(stats.entities, 1);
        assert_eq!(stats.relations, 0, "cạnh phải bị cascade theo thực thể");
        assert_eq!(stats.observations, 1, "chỉ còn quan sát của Private AI");
        Ok(())
    }

    #[test]
    fn deleting_one_observation_keeps_the_entity() -> GraphResult<()> {
        let mut graph = seeded()?;
        let report = graph.forget(
            &[],
            &[ObservationTarget {
                entity: "Vinh".to_string(),
                observations: vec!["Thích trả lời ngắn gọn".to_string()],
            }],
            &[],
        )?;
        assert_eq!(report.observations, 1);
        let rows = graph.entities(Some(&["Vinh".to_string()]), 10, 10)?;
        assert_eq!(rows[0].observations_total, 1);
        Ok(())
    }

    #[test]
    fn deleting_one_relation_keeps_both_ends() -> GraphResult<()> {
        let mut graph = seeded()?;
        let report = graph.forget(&[], &[], &[edge("Vinh", "phát triển", "Private AI")])?;
        assert_eq!(report.relations, 1);
        assert_eq!(graph.stats()?.entities, 2);
        Ok(())
    }

    #[test]
    fn search_finds_by_name_and_by_observation() -> GraphResult<()> {
        let graph = seeded()?;

        let by_name = graph.search("vinh", 10, 10)?;
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].entity.name, "Vinh");
        assert_eq!(by_name[0].matched_by, MatchedBy::Name);

        // "desktop" appears only inside an observation, so this exercises the FTS half.
        let by_body = graph.search("desktop", 10, 10)?;
        assert_eq!(by_body.len(), 1);
        assert_eq!(by_body[0].entity.name, "Private AI");
        assert_eq!(by_body[0].matched_by, MatchedBy::Observation);

        // The type is searchable too, which is how "list every person" works.
        assert_eq!(graph.search("dự án", 10, 10)?.len(), 1);
        assert!(graph.search("không có gì", 10, 10)?.is_empty());
        assert!(graph.search("   ", 10, 10)?.is_empty());
        Ok(())
    }

    #[test]
    fn search_diacritics_and_wildcards_are_not_syntax() -> GraphResult<()> {
        let mut graph = Graph::in_memory()?;
        graph.remember(
            &[
                entity("Q3", "tài liệu", &["Báo cáo đã xong 100% phần một"]),
                entity("Khác", "tài liệu", &["Không liên quan"]),
            ],
            &[],
        )?;
        // A bare `%` would match every row if it reached SQLite unescaped.
        assert!(graph.search("%", 10, 10)?.is_empty());
        // `remove_diacritics 2` means an unaccented query still finds the accented text.
        assert_eq!(graph.search("bao cao", 10, 10)?.len(), 1);
        Ok(())
    }

    #[test]
    fn reads_are_bounded_by_limit_and_by_observations_per_entity() -> GraphResult<()> {
        let mut graph = Graph::in_memory()?;
        let bodies: Vec<String> = (0..50).map(|n| format!("Sự thật số {n}")).collect();
        let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
        for n in 0..30 {
            graph.remember(&[entity(&format!("Thực thể {n}"), "thử", &refs)], &[])?;
        }

        let rows = graph.entities(None, 5, 3)?;
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].observations.len(), 3);
        assert_eq!(rows[0].observations_total, 50);
        // Most recently written first, so a truncated whole-graph read shows the freshest slice.
        assert_eq!(rows[0].name, "Thực thể 29");

        assert_eq!(graph.search("Thực thể", 4, 2)?.len(), 4);
        Ok(())
    }

    #[test]
    fn hydrate_keeps_the_newest_observations_in_writing_order() -> GraphResult<()> {
        let mut graph = Graph::in_memory()?;
        for n in 0..20 {
            graph.remember(&[entity("Vinh", "người", &[&format!("Sự thật {n}")])], &[])?;
        }
        let rows = graph.entities(Some(&["Vinh".to_string()]), 10, 3)?;
        assert_eq!(rows[0].observations_total, 20);
        // The last three written, still oldest-first on the page.
        assert_eq!(
            rows[0].observations,
            vec![
                "Sự thật 17".to_string(),
                "Sự thật 18".to_string(),
                "Sự thật 19".to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn relations_among_hides_dangling_edges() -> GraphResult<()> {
        let graph = seeded()?;
        let only_vinh = graph.entities(Some(&["Vinh".to_string()]), 10, 10)?;
        let ids: Vec<i64> = only_vinh.iter().map(|e| e.id).collect();
        // The edge leaves the requested set, so it must not be reported.
        assert!(graph.relations_among(&ids)?.is_empty());
        assert!(graph.relations_among(&[])?.is_empty());
        Ok(())
    }
}
