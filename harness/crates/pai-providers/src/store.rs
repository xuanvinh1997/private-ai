//! User-configured providers on disk, in their own `provider.db` so a new provider field never migrates
//! old conversation records. This file holds API keys, so it is created `0600` via the open `mode` flag
//! rather than a later `chmod`. One provider list, several role pointers -- see [`Role`].

use std::path::Path;

use pai_llm::{ProviderConfig, ProviderKind};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{ProviderError, Result};

/// File name inside `data_dir`.
pub const DB_FILE: &str = "provider.db";

/// What this provider is currently used for. The roles are fully separate because embedding and chat are
/// different models on different endpoints, and the most useful pairing is cross-wired: embed locally
/// while chatting with a large remote model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Chat,
    Embedding,
    /// Image reading, for OCR of PDF pages with no text layer; its own role because not every chat model can see.
    Vision,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Chat => "chat",
            Role::Embedding => "embedding",
            Role::Vision => "vision",
        }
    }

    /// From the wire string; `None` for an unknown role, since guessing would silently retarget the wrong role.
    pub fn parse(value: &str) -> Option<Role> {
        match value.trim().to_lowercase().as_str() {
            "chat" => Some(Role::Chat),
            "embedding" | "embed" => Some(Role::Embedding),
            "vision" | "ocr" => Some(Role::Vision),
            _ => None,
        }
    }
}

/// A provider as stored on disk. `Debug` is hand-written because printing [`ProviderConfig`] would print
/// the API key; the redacted form is [`pai_llm::ProviderSignature`].
#[derive(Clone, PartialEq, Eq)]
pub struct StoredProvider {
    pub config: ProviderConfig,
    /// This provider's own chat model; per-row, so switching providers cannot send `gpt-4o` to an Ollama host.
    pub model: Option<String>,
    /// This provider's embedding model, unrelated to [`model`]: borrowing the chat name guarantees a 400 on every ingest.
    ///
    /// [`model`]: StoredProvider::model
    pub embedding_model: Option<String>,
    /// Holds the chat role, per [`pai_llm::active_config`]'s selection rule.
    pub active_chat: bool,
    /// Holds the embedding role.
    pub active_embedding: bool,
    /// This provider's image-reading model; empty means unset, and scanned PDFs then report rather than silently indexing nothing.
    pub vision_model: Option<String>,
    /// Holds the vision role.
    pub active_vision: bool,
}

impl std::fmt::Debug for StoredProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredProvider")
            .field("name", &self.config.name)
            .field("enabled", &self.config.enabled)
            .field("signature", &self.config.signature())
            .field("model", &self.model)
            .field("embedding_model", &self.embedding_model)
            .field("vision_model", &self.vision_model)
            .field("active_chat", &self.active_chat)
            .field("active_embedding", &self.active_embedding)
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

    /// Which roles this provider currently holds.
    pub fn holds(&self, role: Role) -> bool {
        match role {
            Role::Chat => self.active_chat,
            Role::Embedding => self.active_embedding,
            Role::Vision => self.active_vision,
        }
    }
}

/// A filled-in form coming down from the UI.
#[derive(Clone, Debug)]
pub struct ProviderInput {
    /// Absent means create.
    pub id: Option<String>,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// `None` keeps the existing key: the UI never receives it back, so treating `None` as "clear" would drop
    /// the key on every rename and only surface as a 401 later. Send `Some("")` to really clear it.
    pub api_key: Option<String>,
    pub enabled: bool,
    /// No "keep existing" semantics: model names are not secret, so the UI always knows and resends the value.
    pub model: Option<String>,
    /// Embedding model, same rule as [`model`]; setting it does not grant the role, since sending documents
    /// somewhere is a decision, not a side effect of typing. Roles change in [`ProviderStore::activate`].
    ///
    /// [`model`]: ProviderInput::model
    pub embedding_model: Option<String>,
    /// Image-reading model for OCR of text-less PDF pages; like [`embedding_model`], setting it does not grant the role.
    ///
    /// [`embedding_model`]: ProviderInput::embedding_model
    pub vision_model: Option<String>,
}

impl ProviderInput {
    /// A form for a new provider, enabled by default -- nobody adds one in order to disable it.
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
            embedding_model: None,
            vision_model: None,
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

    pub fn with_vision_model(mut self, model: impl Into<String>) -> Self {
        self.vision_model = Some(model.into());
        self
    }

    pub fn with_embedding_model(mut self, model: impl Into<String>) -> Self {
        self.embedding_model = Some(model.into());
        self
    }
}

pub trait ProviderStore: Send + Sync + 'static {
    fn list(&self) -> Result<Vec<StoredProvider>>;
    /// A missing `input.id` means create. See [`ProviderInput::api_key`] for the key-retention rule.
    fn save(&self, input: ProviderInput) -> Result<StoredProvider>;
    fn remove(&self, id: &str) -> Result<()>;
    /// Grant a role to a provider; a missing `model` keeps whatever that provider already had for that role.
    fn activate(&self, role: Role, id: &str, model: Option<&str>) -> Result<StoredProvider>;
    fn active(&self, role: Role) -> Result<Option<StoredProvider>>;
}

/// The state row carries `CHECK (id = 0)`: who holds which role is a single fact, and a table that allows
/// two rows will grow two. Pointers duplicate, configurations do not. `ON DELETE SET NULL` is a second line
/// of defence behind [`remove`], for any deletion path written later.
///
/// [`remove`]: ProviderStore::remove
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS providers (
  id              TEXT    PRIMARY KEY,
  name            TEXT    NOT NULL,
  kind            TEXT    NOT NULL,
  base_url        TEXT    NOT NULL,
  api_key         TEXT    NOT NULL,
  enabled         INTEGER NOT NULL,
  model           TEXT,
  embedding_model TEXT,
  vision_model TEXT,
  created_at      INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS provider_state (
  id                  INTEGER PRIMARY KEY CHECK (id = 0),
  active_chat_id      TEXT REFERENCES providers (id) ON DELETE SET NULL,
  active_embedding_id TEXT REFERENCES providers (id) ON DELETE SET NULL,
  active_vision_id    TEXT REFERENCES providers (id) ON DELETE SET NULL
) STRICT;

INSERT OR IGNORE INTO provider_state (id) VALUES (0);

CREATE INDEX IF NOT EXISTS providers_created ON providers (created_at);
";

pub struct SqliteProviderStore {
    conn: std::sync::Mutex<Connection>,
}

impl SqliteProviderStore {
    /// Open the store, creating the file `0600` if absent.
    pub fn open(path: impl AsRef<Path>) -> Result<SqliteProviderStore> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        // Create the empty file before SQLite touches it: `sqlite3_open` honours umask, and a 0644 file holding
        // API keys is already compromised even if chmod-ed a moment later.
        create_private(path)?;
        harden(path)?;
        SqliteProviderStore::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<SqliteProviderStore> {
        SqliteProviderStore::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<SqliteProviderStore> {
        conn.pragma_update(None, "busy_timeout", 5000)?;
        // Migrate before `SCHEMA` with foreign keys off: `CREATE TABLE IF NOT EXISTS` cannot fix an existing
        // old-schema table, and `ALTER TABLE` on a referenced table is where SQLite is fussiest.
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        migrate(&conn)?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(SqliteProviderStore {
            conn: std::sync::Mutex::new(conn),
        })
    }

    /// A poisoned lock means another thread panicked holding it; the connection is still usable, so recover instead of panicking again.
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&guard)
    }
}

/// Lift an older file onto the multi-role schema by probing columns and using `ALTER TABLE`, never rebuilding:
/// this is hand-typed user data including API keys. The old `active_id` is renamed to `active_chat_id`,
/// and the embedding role starts empty because the user was never asked.
fn migrate(conn: &Connection) -> Result<()> {
    if has_table(conn, "providers")? && !has_column(conn, "providers", "embedding_model")? {
        conn.execute_batch("ALTER TABLE providers ADD COLUMN embedding_model TEXT")?;
    }
    // The vision role came last and also starts empty: guessing would send images to a model that cannot see.
    if has_table(conn, "providers")? && !has_column(conn, "providers", "vision_model")? {
        conn.execute_batch("ALTER TABLE providers ADD COLUMN vision_model TEXT")?;
    }
    if !has_table(conn, "provider_state")? {
        return Ok(());
    }
    if has_column(conn, "provider_state", "active_id")?
        && !has_column(conn, "provider_state", "active_chat_id")?
    {
        conn.execute_batch("ALTER TABLE provider_state RENAME COLUMN active_id TO active_chat_id")?;
    }
    if !has_column(conn, "provider_state", "active_embedding_id")? {
        conn.execute_batch(
            "ALTER TABLE provider_state
             ADD COLUMN active_embedding_id TEXT REFERENCES providers (id) ON DELETE SET NULL",
        )?;
    }
    if !has_column(conn, "provider_state", "active_vision_id")? {
        conn.execute_batch(
            "ALTER TABLE provider_state
             ADD COLUMN active_vision_id TEXT REFERENCES providers (id) ON DELETE SET NULL",
        )?;
    }
    Ok(())
}

fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    let found: Option<String> = conn
        .prepare_cached("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")?
        .query_row(params![table], |row| row.get(0))
        .optional()?;
    Ok(found.is_some())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut found = false;
    conn.pragma(None, "table_info", table, |row| {
        if row.get::<_, String>("name")? == column {
            found = true;
        }
        Ok(())
    })?;
    Ok(found)
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
        // Already there: [`harden`] takes care of the permissions.
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(not(unix))]
fn create_private(_path: &Path) -> std::io::Result<()> {
    // Windows has no POSIX permission bits; the profile directory's ACL is all that protects this file.
    Ok(())
}

#[cfg(unix)]
fn harden(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        // Tighten and say so: the key may be exposed, but deleting it here would cost the user a configuration they did not break.
        tracing::warn!(
            path = %path.display(),
            mode = format!("{:o}", mode & 0o777),
            "provider store was world-readable; tightened to 0600"
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

/// One row, not yet knowing which roles it holds.
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProvider> {
    let kind: String = row.get("kind")?;
    let mut config = ProviderConfig::new(
        row.get::<_, String>("id")?,
        row.get::<_, String>("name")?,
        // An unknown `kind` (older build, or hand-edited) falls back to OpenAI-compatible rather than breaking the list.
        ProviderKind::parse(&kind).unwrap_or(ProviderKind::OpenAiCompatible),
        row.get::<_, String>("base_url")?,
    )
    .with_api_key(row.get::<_, String>("api_key")?);
    config.enabled = row.get::<_, i64>("enabled")? != 0;
    Ok(StoredProvider {
        config,
        model: row.get::<_, Option<String>>("model")?,
        embedding_model: row.get::<_, Option<String>>("embedding_model")?,
        vision_model: row.get::<_, Option<String>>("vision_model")?,
        active_chat: false,
        active_embedding: false,
        active_vision: false,
    })
}

const SELECT: &str = "SELECT id, name, kind, base_url, api_key, enabled, model,
                             embedding_model, vision_model
                      FROM providers ORDER BY created_at";

fn all(conn: &Connection) -> Result<Vec<StoredProvider>> {
    let mut stmt = conn.prepare_cached(SELECT)?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The role pointers, read in one go.
struct Pointers {
    chat: Option<String>,
    embedding: Option<String>,
    vision: Option<String>,
}

fn pointers(conn: &Connection) -> Result<Pointers> {
    let mut stmt = conn.prepare_cached(
        "SELECT active_chat_id, active_embedding_id, active_vision_id          FROM provider_state WHERE id = 0",
    )?;
    let found = stmt
        .query_row([], |row| {
            Ok(Pointers {
                chat: row.get(0)?,
                embedding: row.get(1)?,
                vision: row.get(2)?,
            })
        })
        .optional()?;
    Ok(found.unwrap_or(Pointers {
        chat: None,
        embedding: None,
        vision: None,
    }))
}

/// Who holds the chat role, per [`pai_llm::active_config`]; the three fallback tiers are not reimplemented here,
/// at the cost of cloning a list of a few dozen configs.
fn resolve_chat(rows: &[StoredProvider], selected: Option<&str>) -> Option<String> {
    let configs: Vec<ProviderConfig> = rows.iter().map(|row| row.config.clone()).collect();
    pai_llm::active_config(&configs, selected.unwrap_or_default()).map(|config| config.id.clone())
}

/// Who holds the embedding role: exactly the pinned provider, or nobody. No fallback on purpose -- falling back
/// would start shipping documents to a server the user never chose, and "nobody" is a valid answer.
fn resolve_embedding(rows: &[StoredProvider], selected: Option<&str>) -> Option<String> {
    let selected = selected?.trim();
    rows.iter()
        .find(|row| row.config.id == selected)
        .map(|row| row.config.id.clone())
}

fn decorate(mut rows: Vec<StoredProvider>, pointers: &Pointers) -> Vec<StoredProvider> {
    let chat = resolve_chat(&rows, pointers.chat.as_deref());
    let embedding = resolve_embedding(&rows, pointers.embedding.as_deref());
    // The vision role has no succession rule: the pointer is the answer, and empty means nobody holds it.
    let vision = pointers
        .vision
        .as_deref()
        .filter(|want| rows.iter().any(|row| row.config.id == *want))
        .map(str::to_string);
    for row in &mut rows {
        let id = Some(row.config.id.as_str());
        row.active_chat = id == chat.as_deref();
        row.active_embedding = id == embedding.as_deref();
        row.active_vision = id == vision.as_deref();
    }
    rows
}

fn find(conn: &Connection, id: &str) -> Result<StoredProvider> {
    let rows = decorate(all(conn)?, &pointers(conn)?);
    rows.into_iter()
        .find(|row| row.config.id == id)
        .ok_or_else(|| ProviderError::NotFound(id.to_string()))
}

impl ProviderStore for SqliteProviderStore {
    fn list(&self) -> Result<Vec<StoredProvider>> {
        self.with_conn(|conn| Ok(decorate(all(conn)?, &pointers(conn)?)))
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
                        // `COALESCE(?, api_key)` is the "None keeps the key" rule, written in SQL so there is only one write path.
                        "UPDATE providers
                         SET name = ?2, kind = ?3, base_url = ?4,
                             api_key = COALESCE(?5, api_key),
                             enabled = ?6, model = ?7, embedding_model = ?8,
                             vision_model = ?9
                         WHERE id = ?1",
                        params![
                            id,
                            name,
                            input.kind.as_str(),
                            base_url,
                            input.api_key,
                            input.enabled as i64,
                            input.model,
                            input.embedding_model,
                            input.vision_model,
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
                           (id, name, kind, base_url, api_key, enabled, model,
                            embedding_model, vision_model, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            id,
                            name,
                            input.kind.as_str(),
                            base_url,
                            input.api_key.unwrap_or_default(),
                            input.enabled as i64,
                            input.model,
                            input.embedding_model,
                            input.vision_model,
                            now_ms(),
                        ],
                    )?;
                    // The first provider is pinned to the chat role at once; otherwise it exists only via fallback
                    // and silently changes when a second provider is added.
                    conn.execute(
                        "UPDATE provider_state SET active_chat_id = ?1
                         WHERE id = 0 AND active_chat_id IS NULL",
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
            // Deleting the chat provider must name a successor in the same operation, or two consecutive reads could differ.
            let rows = all(conn)?;
            let pointers = pointers(conn)?;
            let heir = resolve_chat(&rows, pointers.chat.as_deref());
            conn.execute(
                "UPDATE provider_state SET active_chat_id = ?1 WHERE id = 0",
                params![heir],
            )?;
            // The embedding role has no successor, only emptiness, but still needs clearing or its pointer dangles.
            let held = resolve_embedding(&rows, pointers.embedding.as_deref());
            if held.is_none() {
                conn.execute(
                    "UPDATE provider_state SET active_embedding_id = NULL WHERE id = 0",
                    [],
                )?;
            }
            Ok(())
        })
    }

    fn activate(&self, role: Role, id: &str, model: Option<&str>) -> Result<StoredProvider> {
        self.with_conn(|conn| {
            let exists: Option<String> = conn
                .prepare_cached("SELECT id FROM providers WHERE id = ?1")?
                .query_row(params![id], |row| row.get(0))
                .optional()?;
            if exists.is_none() {
                return Err(ProviderError::NotFound(id.to_string()));
            }
            // Each role writes its own model column, so granting one role never disturbs another's model name.
            let (column, pointer) = match role {
                Role::Chat => ("model", "active_chat_id"),
                Role::Embedding => ("embedding_model", "active_embedding_id"),
                Role::Vision => ("vision_model", "active_vision_id"),
            };
            if let Some(model) = model {
                conn.execute(
                    &format!("UPDATE providers SET {column} = ?2 WHERE id = ?1"),
                    params![id, model],
                )?;
            }
            conn.execute(
                &format!("UPDATE provider_state SET {pointer} = ?1 WHERE id = 0"),
                params![id],
            )?;
            find(conn, id)
        })
    }

    fn active(&self, role: Role) -> Result<Option<StoredProvider>> {
        self.with_conn(|conn| {
            Ok(decorate(all(conn)?, &pointers(conn)?)
                .into_iter()
                .find(|row| row.holds(role)))
        })
    }
}
