//! The server config store: one JSON file, not a database, because users paste MCP config
//! from third-party docs rather than typing it. It reads both shapes and writes only
//! `mcpServers`; it holds API keys, so it is written `0600` and atomically.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::{ConfigError, McpTransport, ServerConfig};
use crate::hub::{McpHub, Mount};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("không đọc/ghi được kho MCP tại {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("kho MCP tại {0} không phải JSON đọc được: {1}")]
    Malformed(PathBuf, serde_json::Error),
    #[error("không dựng được JSON cho kho MCP: {0}")]
    Encode(serde_json::Error),
    #[error("không có server nào tên `{0}` trong kho")]
    NotFound(String),
    #[error(transparent)]
    Invalid(#[from] ConfigError),
}

/// The user-managed server list, on disk.
pub struct McpStore {
    path: PathBuf,
    /// Every write is read-modify-write; without this lock a concurrent write silently swallows the earlier one.
    writing: Mutex<()>,
}

impl McpStore {
    pub fn open(path: PathBuf) -> McpStore {
        McpStore {
            path,
            writing: Mutex::new(()),
        }
    }

    /// The file path, so the UI can point the user at it for hand editing.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every saved server, disabled ones included; a missing file means none yet, not an error.
    pub fn list(&self) -> Result<Vec<ServerConfig>, StoreError> {
        let Some(text) = self.read()? else {
            return Ok(Vec::new());
        };
        let shape: FileShape = serde_json::from_str(&text)
            .map_err(|err| StoreError::Malformed(self.path.clone(), err))?;
        Ok(shape.into_configs())
    }

    /// Add or replace a server, keyed by name, and validate at the door so a broken config never reaches the file.
    pub fn save(&self, config: ServerConfig) -> Result<(), StoreError> {
        config.validate()?;
        let _writing = self.writing.lock();
        let mut configs = self.list()?;
        configs.retain(|existing| existing.name != config.name);
        configs.push(config);
        self.write(&configs)
    }

    /// `false` means there was nothing to delete, not an error: two clicks on one row must agree.
    pub fn remove(&self, name: &str) -> Result<bool, StoreError> {
        let _writing = self.writing.lock();
        let mut configs = self.list()?;
        let before = configs.len();
        configs.retain(|config| config.name != name);
        if configs.len() == before {
            return Ok(false);
        }
        self.write(&configs)?;
        Ok(true)
    }

    /// Toggle without deleting: a disabled server keeps its token, so re-enabling is a click, not a re-paste.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), StoreError> {
        let _writing = self.writing.lock();
        let mut configs = self.list()?;
        let Some(config) = configs.iter_mut().find(|config| config.name == name) else {
            return Err(StoreError::NotFound(name.to_string()));
        };
        config.enabled = enabled;
        self.write(&configs)
    }

    fn read(&self) -> Result<Option<String>, StoreError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StoreError::Io(self.path.clone(), err)),
        }
    }

    /// Atomic write via a temp file in the same directory then `rename`: a half-written file would lose every server at once.
    fn write(&self, configs: &[ServerConfig]) -> Result<(), StoreError> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(|err| StoreError::Io(dir.to_path_buf(), err))?;

        let shape = FileShape::from_configs(configs);
        let mut body = serde_json::to_vec_pretty(&shape).map_err(StoreError::Encode)?;
        body.push(b'\n');

        let tmp = dir.join(temp_name(&self.path));
        match self.spill(&tmp, &body) {
            Ok(()) => Ok(()),
            Err(err) => {
                // A leftover temp file is litter in the user's data directory, and this litter holds tokens.
                let _ = fs::remove_file(&tmp);
                Err(err)
            }
        }
    }

    fn spill(&self, tmp: &Path, body: &[u8]) -> Result<(), StoreError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // `0600` at creation, not afterwards: in between, the temp file already holds tokens and is world-readable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(tmp)
            .map_err(|err| StoreError::Io(tmp.to_path_buf(), err))?;
        file.write_all(body)
            .map_err(|err| StoreError::Io(tmp.to_path_buf(), err))?;
        // Flush before renaming, or `rename` is atomic only in ordering and the new name points at buffered data.
        file.sync_all()
            .map_err(|err| StoreError::Io(tmp.to_path_buf(), err))?;
        drop(file);
        fs::rename(tmp, &self.path).map_err(|err| StoreError::Io(self.path.clone(), err))
    }
}

/// Temp names must differ between concurrent processes, or the second `create_new` fails for no visible reason.
fn temp_name(path: &Path) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "mcp.json".to_string());
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(".{base}.{}.{seq}.tmp", std::process::id())
}

/// Merge the patch file's `mcp` rows with the user store; the store wins on a name clash, since it is the newer click.
pub fn merge(rows: Vec<ServerConfig>, stored: Vec<ServerConfig>) -> Vec<ServerConfig> {
    let mut by_name: BTreeMap<String, ServerConfig> = BTreeMap::new();
    for config in rows.into_iter().chain(stored) {
        by_name.insert(config.name.clone(), config);
    }
    by_name.into_values().collect()
}

/// The only way to push config onto a running hub, so every change goes through the same [`McpHub::reload`] diff.
pub async fn apply(
    hub: &McpHub,
    store: &McpStore,
    rows: &[ServerConfig],
) -> Result<Vec<(String, Result<Mount, ConfigError>)>, StoreError> {
    let configs = merge(rows.to_vec(), store.list()?);
    Ok(hub.reload(configs).await)
}

/// The on-disk shape, both flavours in one struct, because a file legitimately carries both blocks.
#[derive(Debug, Default, Deserialize, Serialize)]
struct FileShape {
    #[serde(
        default,
        rename = "mcpServers",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    mcp_servers: BTreeMap<String, Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    servers: Vec<ServerConfig>,
}

impl FileShape {
    fn from_configs(configs: &[ServerConfig]) -> FileShape {
        FileShape {
            mcp_servers: configs
                .iter()
                .map(|config| (config.name.clone(), Entry::from_config(config)))
                .collect(),
            servers: Vec::new(),
        }
    }

    fn into_configs(self) -> Vec<ServerConfig> {
        let mut by_name: BTreeMap<String, ServerConfig> = BTreeMap::new();
        for (name, entry) in self.mcp_servers {
            match entry.into_config(&name) {
                Some(config) => {
                    by_name.insert(name, config);
                }
                // Drop one entry, not the file: a paste missing `command` is one bad line.
                None => tracing::warn!(
                    server = %name,
                    "skipping an MCP store entry: neither `command` nor `url` says where to go"
                ),
            }
        }
        // The native shape wins over the pasted one: it says more, so it is the more deliberate declaration.
        for config in self.servers {
            by_name.insert(config.name.clone(), config);
        }
        by_name.into_values().collect()
    }
}

/// One entry in the `mcpServers` block, read more permissively than written: pasted config carries other tools' keys.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct Entry {
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    headers: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    /// Other tools say the same thing inverted; read both, write one, or a pasted file shows as enabled when it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connect_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_retries: Option<u32>,
}

impl Entry {
    fn from_config(config: &ServerConfig) -> Entry {
        let mut entry = Entry {
            enabled: Some(config.enabled),
            connect_timeout_secs: Some(config.connect_timeout_secs),
            max_retries: Some(config.max_retries),
            ..Entry::default()
        };
        match &config.transport {
            McpTransport::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                entry.command = Some(command.clone());
                entry.args = args.clone();
                entry.env = env.clone();
                entry.cwd = cwd.clone();
            }
            McpTransport::Http { url, headers } => {
                entry.url = Some(url.clone());
                entry.headers = headers.clone();
            }
        }
        entry
    }

    /// `None` means the entry never says where it goes.
    fn into_config(self, name: &str) -> Option<ServerConfig> {
        // `url` first: an entry with both is a paste over a paste, and an address is more specific than a leftover command.
        let transport = if let Some(url) = self.url {
            McpTransport::Http {
                url,
                headers: self.headers,
            }
        } else {
            McpTransport::Stdio {
                command: self.command?,
                args: self.args,
                env: self.env,
                cwd: self.cwd,
            }
        };
        // Build from the constructor then swap the transport, so every default comes from [`crate::config`] alone.
        let mut config = ServerConfig::stdio(name, "");
        config.transport = transport;
        config.enabled = self
            .enabled
            .unwrap_or_else(|| !self.disabled.unwrap_or(false));
        if let Some(secs) = self.connect_timeout_secs {
            config.connect_timeout_secs = secs;
        }
        if let Some(retries) = self.max_retries {
            config.max_retries = retries;
        }
        Some(config)
    }
}
