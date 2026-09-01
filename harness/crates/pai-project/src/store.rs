//! Danh sách dự án đã mở.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    /// Tên thư mục. Đổi được, và không phải danh tính.
    pub name: String,
    /// Đường dẫn tuyệt đối đã chuẩn hoá. **Đây mới là danh tính.**
    pub path: String,
    pub last_opened_at: i64,
}

pub struct SqliteProjectStore {
    conn: std::sync::Mutex<Connection>,
}

/// Bảng riêng, tệp riêng, không đụng vào sổ tay phiên.
///
/// Hai thứ có vòng đời khác nhau: một dự án sống lâu hơn mọi phiên của nó, và sổ tay phiên
/// cố ý từ chối mở khi lệch schema. Nhét bảng này vào đó nghĩa là mỗi lần thêm một trường
/// cho dự án là một lần mọi bản ghi hội thoại cũ phải migrate theo.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects (
  id             TEXT    PRIMARY KEY,
  path           TEXT    NOT NULL UNIQUE,
  name           TEXT    NOT NULL,
  last_opened_at INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS projects_recent ON projects (last_opened_at DESC);
";

impl SqliteProjectStore {
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<SqliteProjectStore> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
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

/// Đường dẫn đã chuẩn hoá, và nó phải là một thư mục đang tồn tại.
///
/// Chuẩn hoá ở **cửa vào** chứ không lúc so sánh: hai lối vào cùng một thư mục phải va
/// vào ràng buộc `UNIQUE` của cột `path`, chứ không tạo ra hai hàng rồi mới phát hiện.
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

pub trait ProjectStore: Send + Sync + 'static {
    /// Mới nhất trước — thứ tự người ta nghĩ tới khi mở lại một dự án.
    fn list(&self) -> Result<Vec<Project>>;
    /// Ghi nhận một dự án và đánh dấu vừa mở. Đã có thì cập nhật thời gian, không thêm hàng.
    fn touch(&self, path: &Path) -> Result<Project>;
    fn get(&self, id: &str) -> Result<Project>;
    /// Bỏ khỏi danh sách. **Không đụng tới đĩa** — đó là thư mục của người dùng.
    fn forget(&self, id: &str) -> Result<()>;
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        path: row.get("path")?,
        name: row.get("name")?,
        last_opened_at: row.get("last_opened_at")?,
    })
}

impl ProjectStore for SqliteProjectStore {
    fn list(&self) -> Result<Vec<Project>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, path, name, last_opened_at FROM projects
                 ORDER BY last_opened_at DESC",
            )?;
            let rows = stmt.query_map([], row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn touch(&self, path: &Path) -> Result<Project> {
        let resolved = canonical(path)?;
        let key = resolved.display().to_string();
        let name = resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            // Thư mục gốc không có `file_name`. Hiếm, nhưng một dòng trống trong danh sách
            // thì không bấm được.
            .unwrap_or_else(|| key.clone());

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO projects (id, path, name, last_opened_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET last_opened_at = excluded.last_opened_at,
                                                 name = excluded.name",
                params![uuid::Uuid::now_v7().to_string(), key, name, now_ms()],
            )?;
            let mut stmt = conn.prepare_cached(
                "SELECT id, path, name, last_opened_at FROM projects WHERE path = ?1",
            )?;
            Ok(stmt.query_row(params![key], row)?)
        })
    }

    fn get(&self, id: &str) -> Result<Project> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id, path, name, last_opened_at FROM projects WHERE id = ?1",
            )?;
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
