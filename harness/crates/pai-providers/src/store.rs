//! Provider người dùng đã cấu hình, nằm trên đĩa.
//!
//! Tệp riêng (`provider.db`), không nhét chung vào sổ tay phiên hay danh sách dự án — lý
//! do y hệt `pai-project`: ba thứ có ba vòng đời, và một trường mới cho provider không
//! được kéo theo một lần migrate của mọi bản ghi hội thoại cũ.
//!
//! Khác một điểm quan trọng so với hai kho kia: **tệp này chứa khoá API**. Nó được tạo
//! với quyền `0600` ngay từ lúc sinh ra, theo đúng cách `pai-mcp::token` làm — mở bằng cờ
//! `mode`, không phải `chmod` một nhịp sau khi đã ghi.

use std::path::Path;

use pai_llm::{ProviderConfig, ProviderKind};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{ProviderError, Result};

/// Tên tệp trong `data_dir`.
pub const DB_FILE: &str = "provider.db";

/// Một provider như nó nằm trên đĩa.
///
/// `Debug` viết tay: [`ProviderConfig`] của `pai-llm` derive `Debug` và `api_key` là
/// trường công khai, nên in thẳng nó ra là in cả khoá. Dạng đã che sẵn có tên là
/// [`pai_llm::ProviderSignature`]; đi qua nó thay vì viết lại một bộ che thứ hai.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredProvider {
    pub config: ProviderConfig,
    /// Mô hình đang chọn cho **riêng provider này**. Đổi provider mà quên đổi mô hình là
    /// gửi `gpt-4o` tới một máy Ollama, nên tên mô hình phải đi kèm từng hàng chứ không
    /// nằm ở một ô cấu hình chung.
    pub model: Option<String>,
    /// Cái đang hoạt động, theo đúng luật chọn của [`pai_llm::active_config`].
    pub active: bool,
}

impl std::fmt::Debug for StoredProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredProvider")
            .field("name", &self.config.name)
            .field("enabled", &self.config.enabled)
            .field("signature", &self.config.signature())
            .field("model", &self.model)
            .field("active", &self.active)
            .finish()
    }
}

impl StoredProvider {
    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn has_key(&self) -> bool {
        !self.config.api_key.is_empty()
    }
}

/// Một biểu mẫu đã điền, từ giao diện đi xuống.
#[derive(Clone, Debug)]
pub struct ProviderInput {
    /// Vắng nghĩa là tạo mới.
    pub id: Option<String>,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// **`None` = giữ nguyên khoá đang có.** Giao diện không bao giờ nhận lại khoá đã
    /// lưu, nên nó không có gì để gửi lại; nếu `None` bị hiểu là "xoá" thì mỗi lần người
    /// dùng sửa cái tên provider là một lần khoá bốc hơi, và triệu chứng chỉ lộ ra ở
    /// request kế tiếp dưới dạng 401. Muốn xoá thật thì gửi `Some("")`.
    pub api_key: Option<String>,
    pub enabled: bool,
    /// Không có ngữ nghĩa "giữ nguyên": tên mô hình không phải bí mật, giao diện luôn
    /// biết giá trị hiện tại và luôn gửi lại được nó.
    pub model: Option<String>,
}

impl ProviderInput {
    /// Biểu mẫu cho một provider mới, đã bật sẵn — không ai thêm một provider để tắt nó.
    pub fn create(
        name: impl Into<String>,
        kind: ProviderKind,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            name: name.into(),
            kind,
            base_url: base_url.into(),
            api_key: None,
            enabled: true,
            model: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

pub trait ProviderStore: Send + Sync + 'static {
    fn list(&self) -> Result<Vec<StoredProvider>>;
    /// `input.id` vắng = tạo mới. Xem [`ProviderInput::api_key`] cho luật giữ khoá.
    fn save(&self, input: ProviderInput) -> Result<StoredProvider>;
    fn remove(&self, id: &str) -> Result<()>;
    /// Ghim provider đang hoạt động. `model` vắng = giữ nguyên mô hình đã chọn của nó.
    fn activate(&self, id: &str, model: Option<&str>) -> Result<StoredProvider>;
    fn active(&self) -> Result<Option<StoredProvider>>;
}

/// Hàng trạng thái có `CHECK (id = 0)`: "provider nào đang hoạt động" là một sự thật duy
/// nhất, và một bảng cho phép hai hàng là một bảng sẽ có hai hàng.
///
/// `ON DELETE SET NULL` là lớp phòng thủ thứ hai chứ không phải lớp thứ nhất — [`remove`]
/// tự chọn người kế nhiệm. Nó có mặt để một đường xoá nào đó viết sau này cũng không để
/// lại một `active_id` trỏ vào hư không.
///
/// [`remove`]: ProviderStore::remove
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS providers (
  id         TEXT    PRIMARY KEY,
  name       TEXT    NOT NULL,
  kind       TEXT    NOT NULL,
  base_url   TEXT    NOT NULL,
  api_key    TEXT    NOT NULL,
  enabled    INTEGER NOT NULL,
  model      TEXT,
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS provider_state (
  id        INTEGER PRIMARY KEY CHECK (id = 0),
  active_id TEXT REFERENCES providers (id) ON DELETE SET NULL
) STRICT;

INSERT OR IGNORE INTO provider_state (id, active_id) VALUES (0, NULL);

CREATE INDEX IF NOT EXISTS providers_created ON providers (created_at);
";

pub struct SqliteProviderStore {
    conn: std::sync::Mutex<Connection>,
}

impl SqliteProviderStore {
    /// Mở kho, tạo tệp với quyền `0600` nếu chưa có.
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProviderStore> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        // Tạo tệp rỗng **trước** khi SQLite chạm vào nó: `sqlite3_open` tạo tệp theo umask,
        // và một tệp 0644 chứa khoá API là một tệp đã hỏng dù có `chmod` ngay sau đó.
        // Một tệp rỗng là một cơ sở dữ liệu SQLite hợp lệ, nên bước này không mất gì.
        create_private(path)?;
        harden(path)?;
        SqliteProviderStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProviderStore> {
        SqliteProviderStore::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<SqliteProviderStore> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
        Ok(SqliteProviderStore {
            conn: std::sync::Mutex::new(conn),
        })
    }

    /// Khoá nhiễm độc nghĩa là một luồng khác đã hoảng khi đang giữ nó. Kết nối vẫn dùng
    /// được — mọi thay đổi đi qua một câu lệnh SQL trọn vẹn — nên lấy lại mà dùng thay vì
    /// lan truyền thêm một cú hoảng nữa.
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&guard)
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        // Đã có sẵn: [`harden`] lo phần quyền.
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(not(unix))]
fn create_private(_path: &Path) -> std::io::Result<()> {
    // Windows không có bit quyền kiểu POSIX; ACL của thư mục hồ sơ người dùng là thứ duy
    // nhất che tệp này. Nói ra ở đây thay vì để im lặng trông như đã xong.
    Ok(())
}

#[cfg(unix)]
fn harden(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        // Siết lại và kêu một tiếng. Khoá có thể đã lộ, nhưng xoá nó ở đây thì người dùng
        // mất cấu hình vì một chuyện họ không gây ra; cái họ cần là một dòng đọc được để
        // tự quyết định đổi khoá.
        tracing::warn!(
            path = %path.display(),
            mode = format!("{:o}", mode & 0o777),
            "kho provider đang mở cho người khác đọc; đã siết về 0600"
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn harden(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Một hàng, chưa biết mình có đang hoạt động không.
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProvider> {
    let kind: String = row.get("kind")?;
    let mut config = ProviderConfig::new(
        row.get::<_, String>("id")?,
        row.get::<_, String>("name")?,
        // Một `kind` lạ trong cơ sở dữ liệu (bản cũ hơn, hoặc sửa tay) rơi về
        // OpenAI-compatible thay vì làm hỏng cả danh sách: đó là hình dạng phổ biến nhất,
        // và người dùng sửa được nó trên màn hình.
        ProviderKind::parse(&kind).unwrap_or(ProviderKind::OpenAiCompatible),
        row.get::<_, String>("base_url")?,
    )
    .with_api_key(row.get::<_, String>("api_key")?);
    config.enabled = row.get::<_, i64>("enabled")? != 0;
    Ok(StoredProvider {
        config,
        model: row.get::<_, Option<String>>("model")?,
        active: false,
    })
}

const SELECT: &str = "SELECT id, name, kind, base_url, api_key, enabled, model
                      FROM providers ORDER BY created_at";

fn all(conn: &Connection) -> Result<Vec<StoredProvider>> {
    let mut stmt = conn.prepare_cached(SELECT)?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn selected_id(conn: &Connection) -> Result<Option<String>> {
    let mut stmt = conn.prepare_cached("SELECT active_id FROM provider_state WHERE id = 0")?;
    Ok(stmt
        .query_row([], |row| row.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

/// Ai đang hoạt động, theo [`pai_llm::active_config`].
///
/// Không tự viết lại ba tầng dự phòng ở đây: luật ấy đã có một bản, và hai bản của cùng
/// một luật là hai bản sẽ lệch nhau. Cái giá là một lần nhân bản danh sách cấu hình, trên
/// một danh sách dài cỡ chục hàng.
fn resolve_active(rows: &[StoredProvider], selected: Option<&str>) -> Option<String> {
    let configs: Vec<ProviderConfig> = rows.iter().map(|row| row.config.clone()).collect();
    pai_llm::active_config(&configs, selected.unwrap_or_default()).map(|config| config.id.clone())
}

fn decorate(mut rows: Vec<StoredProvider>, selected: Option<&str>) -> Vec<StoredProvider> {
    let active = resolve_active(&rows, selected);
    for row in &mut rows {
        row.active = Some(row.config.id.as_str()) == active.as_deref();
    }
    rows
}

fn find(conn: &Connection, id: &str) -> Result<StoredProvider> {
    let rows = decorate(all(conn)?, selected_id(conn)?.as_deref());
    rows.into_iter()
        .find(|row| row.config.id == id)
        .ok_or_else(|| ProviderError::NotFound(id.to_string()))
}

impl ProviderStore for SqliteProviderStore {
    fn list(&self) -> Result<Vec<StoredProvider>> {
        self.with_conn(|conn| Ok(decorate(all(conn)?, selected_id(conn)?.as_deref())))
    }

    fn save(&self, input: ProviderInput) -> Result<StoredProvider> {
        let name = input.name.trim().to_string();
        let base_url = input.base_url.trim().trim_end_matches('/').to_string();
        if name.is_empty() {
            return Err(ProviderError::Invalid("provider phải có tên".into()));
        }
        if base_url.is_empty() {
            return Err(ProviderError::Invalid(
                "provider phải có địa chỉ máy chủ".into(),
            ));
        }

        self.with_conn(|conn| {
            let id = match input
                .id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            {
                Some(id) => {
                    let touched = conn.execute(
                        // `COALESCE(?, api_key)` **là** luật "None thì giữ nguyên", viết
                        // ra bằng SQL chứ không bằng một nhánh `if` ở tầng trên: chỗ nào
                        // cũng ghi được vào bảng này, và chỉ có một câu lệnh ghi.
                        "UPDATE providers
                         SET name = ?2, kind = ?3, base_url = ?4,
                             api_key = COALESCE(?5, api_key),
                             enabled = ?6, model = ?7
                         WHERE id = ?1",
                        params![
                            id,
                            name,
                            input.kind.as_str(),
                            base_url,
                            input.api_key,
                            input.enabled as i64,
                            input.model,
                        ],
                    )?;
                    if touched == 0 {
                        return Err(ProviderError::NotFound(id.to_string()));
                    }
                    id.to_string()
                }
                None => {
                    let id = uuid::Uuid::now_v7().to_string();
                    conn.execute(
                        "INSERT INTO providers
                           (id, name, kind, base_url, api_key, enabled, model, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            id,
                            name,
                            input.kind.as_str(),
                            base_url,
                            input.api_key.unwrap_or_default(),
                            input.enabled as i64,
                            input.model,
                            now_ms(),
                        ],
                    )?;
                    // Provider đầu tiên được ghim luôn. Không có bước này thì cái đang
                    // hoạt động chỉ tồn tại nhờ tầng dự phòng, và nó lặng lẽ đổi khi
                    // người dùng thêm provider thứ hai.
                    conn.execute(
                        "UPDATE provider_state SET active_id = ?1
                         WHERE id = 0 AND active_id IS NULL",
                        params![id],
                    )?;
                    id
                }
            };
            find(conn, &id)
        })
    }

    fn remove(&self, id: &str) -> Result<()> {
        self.with_conn(|conn| {
            let touched = conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
            if touched == 0 {
                return Err(ProviderError::NotFound(id.to_string()));
            }
            // Xoá cái đang hoạt động thì phải có người kế nhiệm ngay trong cùng thao tác.
            // Để `active_id` là NULL rồi trông chờ tầng dự phòng ở lần đọc sau nghĩa là
            // hai lần đọc liên tiếp có thể cho hai câu trả lời khác nhau.
            let rows = all(conn)?;
            let heir = resolve_active(&rows, selected_id(conn)?.as_deref());
            conn.execute(
                "UPDATE provider_state SET active_id = ?1 WHERE id = 0",
                params![heir],
            )?;
            Ok(())
        })
    }

    fn activate(&self, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        self.with_conn(|conn| {
            let exists: Option<String> = conn
                .prepare_cached("SELECT id FROM providers WHERE id = ?1")?
                .query_row(params![id], |row| row.get(0))
                .optional()?;
            if exists.is_none() {
                return Err(ProviderError::NotFound(id.to_string()));
            }
            if let Some(model) = model {
                conn.execute(
                    "UPDATE providers SET model = ?2 WHERE id = ?1",
                    params![id, model],
                )?;
            }
            conn.execute(
                "UPDATE provider_state SET active_id = ?1 WHERE id = 0",
                params![id],
            )?;
            find(conn, id)
        })
    }

    fn active(&self) -> Result<Option<StoredProvider>> {
        self.with_conn(|conn| {
            Ok(decorate(all(conn)?, selected_id(conn)?.as_deref())
                .into_iter()
                .find(|row| row.active))
        })
    }
}
