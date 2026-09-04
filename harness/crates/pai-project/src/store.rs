//! The list of opened projects.

use std::path::{Path, PathBuf};

use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0} không phải một thư mục")]
    NotADirectory(PathBuf),
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(PathBuf, String),
    #[error("không có dự án nào tên `{0}`")]
    NotFound(String),
}

type Result<T> = std::result::Result<T, ProjectError>;

/// Source code or documents; picks the plugin tier to mount, so the user states it rather than it being guessed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    /// The default, and what every migrated row gets: before this change all projects were code.
    #[default]
    Code,
    Docs,
}

impl ProjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectKind::Code => "code",
            ProjectKind::Docs => "docs",
        }
    }
}

impl ToSql for ProjectKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for ProjectKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<ProjectKind> {
        match value.as_str()? {
            "docs" => Ok(ProjectKind::Docs),
            // An unknown kind reads back as `Code`: a wrong label is one click to fix, a dropped row loses a project.
            _ => Ok(ProjectKind::Code),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    /// The directory name. Changeable, and not the identity.
    pub name: String,
    /// The canonical absolute path. **This is the identity.**
    pub path: String,
    pub last_opened_at: i64,
    pub kind: ProjectKind,
    /// Clone URL, `None` for an already-local directory; stored, not read from `git remote`, so a deleted `.git` keeps it.
    pub origin: Option<String>,
}

pub struct SqliteProjectStore {
    conn: std::sync::Mutex<Connection>,
}

/// Its own file, away from the session journal: that one refuses to open on schema drift, and projects outlive sessions.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects (
  id             TEXT    PRIMARY KEY,
  path           TEXT    NOT NULL UNIQUE,
  name           TEXT    NOT NULL,
  last_opened_at INTEGER NOT NULL,
  kind           TEXT    NOT NULL DEFAULT 'code',
  origin         TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS projects_recent ON projects (last_opened_at DESC);
";

/// One column list, because four queries have to return the same shape for [`row`].
const COLUMNS: &str = "id, path, name, last_opened_at, kind, origin";

impl SqliteProjectStore {
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open_in_memory()?)
    }

    /// Build the store on an already-open `Connection`; public so a migration test can seed the old schema first.
    pub fn from_connection(conn: Connection) -> Result<SqliteProjectStore> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(SqliteProjectStore {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&guard)
    }
}

/// Add missing columns in place — unlike the index or the journal, this list cannot be rebuilt from anywhere else.
fn migrate(conn: &Connection) -> Result<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>("name"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has = |name: &str| existing.iter().any(|column| column == name);

    // `kind='code'`/`origin=NULL` are not placeholders: old projects really were local source directories.
    if !has("kind") {
        conn.execute(
            "ALTER TABLE projects ADD COLUMN kind TEXT NOT NULL DEFAULT 'code'",
            [],
        )?;
    }
    if !has("origin") {
        conn.execute("ALTER TABLE projects ADD COLUMN origin TEXT", [])?;
    }
    Ok(())
}

/// The canonical path, which must be an existing directory; resolved on entry so aliases collide on `UNIQUE(path)`.
pub fn canonical(path: &Path) -> Result<PathBuf> {
    let resolved = path
        .canonicalize()
        .map_err(|err| ProjectError::Unresolvable(path.to_path_buf(), err.to_string()))?;
    if !resolved.is_dir() {
        return Err(ProjectError::NotADirectory(resolved));
    }
    Ok(resolved)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// The row key and the displayed name, derived from the canonical path.
fn identity(resolved: &Path) -> (String, String) {
    let key = resolved.display().to_string();
    let name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        // The root directory has no `file_name`, and a blank row is not clickable.
        .unwrap_or_else(|| key.clone());
    (key, name)
}

pub trait ProjectStore: Send + Sync + 'static {
    /// Most recent first — the order people think in when reopening a project.
    fn list(&self) -> Result<Vec<Project>>;
    /// Record a project and mark it just-opened; upserts on path and never changes the kind.
    fn touch(&self, path: &Path) -> Result<Project>;
    /// Record a new project with an **explicit** kind, and where it was cloned from.
    fn create(&self, path: &Path, kind: ProjectKind, origin: Option<&str>) -> Result<Project>;
    /// Change a project's kind: `touch` preserves it, so a wrongly recorded repo has no other way back to its tools.
    fn set_kind(&self, id: &str, kind: ProjectKind) -> Result<Project>;
    fn get(&self, id: &str) -> Result<Project>;
    /// Drop from the list. **Does not touch the disk** — that is the user's directory.
    fn forget(&self, id: &str) -> Result<()>;
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        path: row.get("path")?,
        name: row.get("name")?,
        last_opened_at: row.get("last_opened_at")?,
        kind: row.get("kind")?,
        origin: row.get("origin")?,
    })
}

impl SqliteProjectStore {
    fn by_path(conn: &Connection, key: &str) -> Result<Project> {
        let mut stmt =
            conn.prepare_cached(&format!("SELECT {COLUMNS} FROM projects WHERE path = ?1"))?;
        Ok(stmt.query_row(params![key], row)?)
    }
}

impl ProjectStore for SqliteProjectStore {
    fn list(&self) -> Result<Vec<Project>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM projects ORDER BY last_opened_at DESC"
            ))?;
            let rows = stmt.query_map([], row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn touch(&self, path: &Path) -> Result<Project> {
        let resolved = canonical(path)?;
        let (key, name) = identity(&resolved);

        self.with_conn(|conn| {
            // `DO UPDATE` omits `kind`/`origin`: setting them here would silently turn a document project back into code.
            conn.execute(
                "INSERT INTO projects (id, path, name, last_opened_at, kind, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)
                 ON CONFLICT(path) DO UPDATE SET last_opened_at = excluded.last_opened_at,
                                                 name = excluded.name",
                params![
                    uuid::Uuid::now_v7().to_string(),
                    key,
                    name,
                    now_ms(),
                    ProjectKind::Code
                ],
            )?;
            SqliteProjectStore::by_path(conn, &key)
        })
    }

    fn create(&self, path: &Path, kind: ProjectKind, origin: Option<&str>) -> Result<Project> {
        let resolved = canonical(path)?;
        let (key, name) = identity(&resolved);

        self.with_conn(|conn| {
            // Unlike `touch`, the stated kind wins; `origin` is `COALESCE`d so re-adding a clone keeps its URL.
            conn.execute(
                "INSERT INTO projects (id, path, name, last_opened_at, kind, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET last_opened_at = excluded.last_opened_at,
                                                 name = excluded.name,
                                                 kind = excluded.kind,
                                                 origin = COALESCE(excluded.origin, origin)",
                params![
                    uuid::Uuid::now_v7().to_string(),
                    key,
                    name,
                    now_ms(),
                    kind,
                    origin
                ],
            )?;
            SqliteProjectStore::by_path(conn, &key)
        })
    }

    fn set_kind(&self, id: &str, kind: ProjectKind) -> Result<Project> {
        self.with_conn(|conn| {
            let touched = conn.execute(
                "UPDATE projects SET kind = ?2 WHERE id = ?1",
                params![id, kind],
            )?;
            if touched == 0 {
                return Err(ProjectError::NotFound(id.to_string()));
            }
            let mut stmt =
                conn.prepare_cached(&format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"))?;
            Ok(stmt.query_row(params![id], row)?)
        })
    }

    fn get(&self, id: &str) -> Result<Project> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare_cached(&format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"))?;
            stmt.query_row(params![id], row)
                .optional()?
                .ok_or_else(|| ProjectError::NotFound(id.to_string()))
        })
    }

    fn forget(&self, id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let touched = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
            if touched == 0 {
                return Err(ProjectError::NotFound(id.to_string()));
            }
            Ok(())
        })
    }
}
