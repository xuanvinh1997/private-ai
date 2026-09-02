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

/// Source code, or a stack of documents.
///
/// Not a label for filtering a list: the kind decides **which plugin tier gets mounted**.
/// Which is why the kind is always something the user states when adding the project,
/// never something inferred from the directory's contents — guessing a document folder to
/// be source code hands command execution to a place full of files strangers sent in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    /// The default, and also what every old row gets on migration: before this change
    /// every project was source code.
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
            // An unknown value — a later build wrote a third kind and the user reopened
            // an older one — reads back as `Code` rather than failing the whole call.
            // Losing a label is one click to fix; rejecting the row loses a project from
            // the list, and this list cannot be rebuilt from anywhere.
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
    /// The URL it was cloned from; `None` means a directory that was already on the
    /// machine. Stored rather than asking `git remote` on every render: a project whose
    /// remote was renamed, or whose `.git` was deleted, still has to remember where it
    /// came from.
    pub origin: Option<String>,
}

pub struct SqliteProjectStore {
    conn: std::sync::Mutex<Connection>,
}

/// Its own table, its own file, nowhere near the session journal.
///
/// The two have different lifetimes: a project outlives every session of it, and the
/// session journal deliberately refuses to open on a schema mismatch. Putting this table
/// in there would mean every new project field forces every old conversation record to
/// migrate along with it.
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

/// One place lists the columns, because four queries have to return the same shape for
/// [`row`].
const COLUMNS: &str = "id, path, name, last_opened_at, kind, origin";

impl SqliteProjectStore {
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open_in_memory()?)
    }

    /// Build the store on an already-open `Connection`.
    ///
    /// Public because it is the only way to write a migration test: you have to be able to
    /// build a database on the **old** schema and then open the store on top of it.
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

/// Add missing columns to an existing table, **in place**.
///
/// The three SQLite stores in this tree handle schema drift three different ways, and the
/// differences are deliberate. `pai-index` **rebuilds from scratch**, because the index can
/// be regenerated from source. `pai-session` **refuses to open**, because one silent
/// migration corrupting the journal is permanent and there is no other copy to compare
/// against. The project list is in neither group: it is something the user typed in one
/// line at a time, no source can rebuild it, and losing it means opening the application to
/// an empty list. So here — and only here — we migrate.
///
/// The `CREATE TABLE IF NOT EXISTS` above silently skips an existing table, even one that
/// is missing columns; so we have to ask `PRAGMA table_info` rather than infer anything
/// from the statement having run cleanly.
fn migrate(conn: &Connection) -> Result<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>("name"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has = |name: &str| existing.iter().any(|column| column == name);

    // Old rows default to `kind='code'`, `origin=NULL`: before this change every project
    // was source code and every one was a directory already on the machine, so these are
    // not placeholder values — they are true of what the user actually has.
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

/// The canonical path, which must be an existing directory.
///
/// Canonicalised at the **entrance**, not at comparison time: two ways into the same
/// directory have to collide with the `UNIQUE` constraint on `path`, rather than creating
/// two rows and being noticed afterwards.
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
        // The root directory has no `file_name`. Rare, but a blank row in the list is not
        // clickable.
        .unwrap_or_else(|| key.clone());
    (key, name)
}

pub trait ProjectStore: Send + Sync + 'static {
    /// Most recent first — the order people think in when reopening a project.
    fn list(&self) -> Result<Vec<Project>>;
    /// Record a project and mark it just-opened. If it exists, update the timestamp
    /// rather than adding a row.
    ///
    /// **Does not change the kind.** This path holds only a directory path; it does not
    /// know what kind the user wants — see the setter below for why that matters.
    fn touch(&self, path: &Path) -> Result<Project>;
    /// Record a new project with an **explicit** kind, and where it was cloned from.
    fn create(&self, path: &Path, kind: ProjectKind, origin: Option<&str>) -> Result<Project>;
    /// Change an existing project's kind.
    ///
    /// Necessary because the kind is set **once** at record time and `touch` deliberately
    /// preserves it — so a directory recorded as the wrong kind has no other way out. That
    /// is a real dead end: a source repo accidentally recorded as a document library would
    /// never have `read`, `grep` or `bash` again, and all the user would see is the
    /// assistant saying it has no tools.
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
            // The `DO UPDATE` deliberately mentions neither `kind` nor `origin`. A new
            // row created by this path is source code (the column default), but an existing
            // row keeps whatever kind it had. Adding `kind = excluded.kind` here would turn
            // every document project into a source project on its next reopen — silently,
            // with no notice, only surfacing when command-running tools suddenly appear in
            // a folder full of PDFs that strangers sent in.
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
            // The opposite of `touch`: here the user just stated the kind, so the new
            // kind wins. `origin` gets `COALESCE` — manually re-adding a directory that was
            // cloned must not erase where it came from, because the manual path does not
            // know the URL.
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
