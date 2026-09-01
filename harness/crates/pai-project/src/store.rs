//! Danh sách dự án đã mở.

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

/// Mã nguồn, hay một chồng tài liệu.
///
/// Không phải một nhãn để lọc danh sách: loại quyết định **tầng plugin nào được cắm**.
/// Vì thế loại luôn là thứ người dùng nói ra lúc thêm dự án, không phải thứ suy ra từ
/// nội dung thư mục — đoán nhầm một thư mục tài liệu thành mã nguồn là cấp quyền chạy
/// lệnh cho một chỗ toàn tệp người ngoài gửi tới.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    /// Mặc định, và cũng là giá trị mọi hàng cũ nhận khi migrate: trước đợt này mọi dự
    /// án đều là mã nguồn.
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
            // Một giá trị lạ — bản sau ghi xuống một loại thứ ba rồi người dùng mở lại
            // bản cũ — đọc chệch về `Code` chứ không làm hỏng cả lời gọi. Mất một cái
            // nhãn thì sửa được bằng một cú bấm; từ chối cả hàng thì mất một dự án khỏi
            // danh sách, mà danh sách này không dựng lại được từ đâu.
            _ => Ok(ProjectKind::Code),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    /// Tên thư mục. Đổi được, và không phải danh tính.
    pub name: String,
    /// Đường dẫn tuyệt đối đã chuẩn hoá. **Đây mới là danh tính.**
    pub path: String,
    pub last_opened_at: i64,
    pub kind: ProjectKind,
    /// URL đã clone về; `None` là thư mục vốn có sẵn trên máy. Lưu lại chứ không hỏi
    /// `git remote` mỗi lần vẽ danh sách: một dự án bị đổi tên remote, hay bị xoá `.git`,
    /// vẫn phải nhớ được nó từ đâu tới.
    pub origin: Option<String>,
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
  last_opened_at INTEGER NOT NULL,
  kind           TEXT    NOT NULL DEFAULT 'code',
  origin         TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS projects_recent ON projects (last_opened_at DESC);
";

/// Một chỗ duy nhất liệt kê cột, vì bốn câu truy vấn phải trả về cùng hình dạng cho [`row`].
const COLUMNS: &str = "id, path, name, last_opened_at, kind, origin";

impl SqliteProjectStore {
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProjectStore> {
        SqliteProjectStore::from_connection(Connection::open_in_memory()?)
    }

    /// Dựng kho trên một `Connection` đã mở sẵn.
    ///
    /// Công khai vì đây là cách duy nhất viết được test cho migration: phải dựng được một
    /// cơ sở dữ liệu theo schema **cũ** rồi mở kho lên trên chính nó.
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

/// Thêm cột còn thiếu vào một bảng đã có, **tại chỗ**.
///
/// Ba kho SQLite trong cây này xử lý schema lệch theo ba cách khác nhau, và khác nhau có
/// chủ ý. `pai-index` **dựng lại từ đầu**, vì chỉ mục sinh ra được từ mã nguồn. `pai-session`
/// **từ chối mở**, vì một lần migrate ngầm làm hỏng sổ là hỏng vĩnh viễn và không có bản
/// nào để đối chiếu. Danh sách dự án không thuộc nhóm nào: nó là thứ người dùng tự gõ vào
/// từng dòng một, không có nguồn nào dựng lại được, và mất nó nghĩa là mở ứng dụng lên
/// thấy một danh sách trống. Nên chỗ này — và chỉ chỗ này — migrate.
///
/// `CREATE TABLE IF NOT EXISTS` ở trên lặng lẽ bỏ qua một bảng đã tồn tại, kể cả khi bảng
/// đó thiếu cột; nên phải hỏi `PRAGMA table_info` chứ không suy từ việc câu lệnh chạy trót lọt.
fn migrate(conn: &Connection) -> Result<()> {
    let existing: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>("name"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has = |name: &str| existing.iter().any(|column| column == name);

    // Hàng cũ mặc định `kind='code'`, `origin=NULL`: trước đợt này mọi dự án đều là mã
    // nguồn và đều là thư mục vốn có trên máy, nên đó không phải một giá trị đặt cho có,
    // nó đúng với những gì người dùng thật sự đang có.
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

/// Khoá hàng và tên hiện, suy từ đường dẫn đã chuẩn hoá.
fn identity(resolved: &Path) -> (String, String) {
    let key = resolved.display().to_string();
    let name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        // Thư mục gốc không có `file_name`. Hiếm, nhưng một dòng trống trong danh sách
        // thì không bấm được.
        .unwrap_or_else(|| key.clone());
    (key, name)
}

pub trait ProjectStore: Send + Sync + 'static {
    /// Mới nhất trước — thứ tự người ta nghĩ tới khi mở lại một dự án.
    fn list(&self) -> Result<Vec<Project>>;
    /// Ghi nhận một dự án và đánh dấu vừa mở. Đã có thì cập nhật thời gian, không thêm hàng.
    ///
    /// **Không đổi loại.** Đường này chỉ có đường dẫn trong tay, nó không biết người dùng
    /// muốn dự án này là loại gì — xem cài đặt để biết vì sao đó là chuyện sống còn.
    fn touch(&self, path: &Path) -> Result<Project>;
    /// Ghi nhận một dự án mới với loại **tường minh**, và chỗ nó được clone về từ đâu.
    fn create(&self, path: &Path, kind: ProjectKind, origin: Option<&str>) -> Result<Project>;
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
            // `DO UPDATE` cố tình **không** nhắc tới `kind` lẫn `origin`. Một hàng mới do
            // đường này tạo ra là mã nguồn (mặc định của cột), nhưng một hàng đã có thì
            // giữ nguyên loại của nó. Viết thêm `kind = excluded.kind` vào đây là biến
            // mọi dự án tài liệu thành dự án mã nguồn ở lần mở lại tiếp theo — im lặng,
            // không thông báo, và chỉ lộ ra khi tool chạy lệnh bỗng xuất hiện trong một
            // thư mục toàn PDF người ngoài gửi tới.
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
            // Ngược với `touch`: ở đây người dùng vừa nói ra loại, nên loại mới thắng.
            // `origin` thì `COALESCE` — thêm lại bằng tay một thư mục đã clone về không
            // được xoá mất chỗ nó từ đâu tới, vì đường thêm bằng tay không biết URL.
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
