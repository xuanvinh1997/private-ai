//! Config for a third-party server, and the names it may not carry.
//! The server name sits between `ext.` and the remote tool name, so it is part of a tool's
//! identity: an unchecked name yields an unchecked tool. See [`ServerConfig::validate`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long before a server that never answers `initialize` counts as absent; long enough for a first `npx` download.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Reconnect attempts before giving up; unbounded retries on a mistyped command spawn processes forever.
pub const DEFAULT_MAX_RETRIES: u32 = 5;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("tên server không được rỗng")]
    EmptyName,
    #[error(
        "tên server `{0}` chỉ được gồm chữ ASCII, số, `-` và `_`: nó là một phần của danh tính tool"
    )]
    BadName(String),
    #[error("tên server `{0}` chứa `__`, thứ phá mất phép chiếu tên sang dạng wire")]
    WireCollision(String),
    #[error("server `{0}` khai transport stdio nhưng không có lệnh để chạy")]
    EmptyCommand(String),
    #[error("server `{0}` khai url `{1}`: chỉ chấp nhận http:// hoặc https://")]
    BadUrl(String, String),
    /// Names what is missing: the user is looking at a form and needs to know which field is blank.
    #[error("mục `{0}` trong danh mục còn thiếu giá trị bắt buộc: {1}")]
    MissingValue(String, String),
}

/// How to reach a server: two ways, exactly the two the spec defines.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpTransport {
    /// A child process speaking JSON-RPC over stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Added to the child's environment rather than replacing it.
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Streamable HTTP.
    Http {
        url: String,
        /// Headers sent with every request — where a user token goes.
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// A third-party server, exactly as the user declared it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerConfig {
    /// The middle component of `ext.<name>.<tool>`.
    pub name: String,
    #[serde(flatten)]
    pub transport: McpTransport,
    /// Turn a server off without deleting its configuration.
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn yes() -> bool {
    true
}

fn default_connect_timeout_secs() -> u64 {
    CONNECT_TIMEOUT.as_secs()
}

fn default_max_retries() -> u32 {
    DEFAULT_MAX_RETRIES
}

impl ServerConfig {
    /// A stdio server with default settings.
    pub fn stdio(name: impl Into<String>, command: impl Into<String>) -> ServerConfig {
        ServerConfig {
            name: name.into(),
            transport: McpTransport::Stdio {
                command: command.into(),
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
            },
            enabled: true,
            connect_timeout_secs: CONNECT_TIMEOUT.as_secs(),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    /// A streamable-HTTP server with default settings.
    pub fn http(name: impl Into<String>, url: impl Into<String>) -> ServerConfig {
        ServerConfig {
            name: name.into(),
            transport: McpTransport::Http {
                url: url.into(),
                headers: BTreeMap::new(),
            },
            enabled: true,
            connect_timeout_secs: CONNECT_TIMEOUT.as_secs(),
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs)
    }

    /// Check before dialing: the name must be non-empty, dot-free and `__`-free, since it is part of every tool's identity.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.name.is_empty() {
            return Err(ConfigError::EmptyName);
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ConfigError::BadName(self.name.clone()));
        }
        if self.name.contains("__") {
            return Err(ConfigError::WireCollision(self.name.clone()));
        }
        match &self.transport {
            McpTransport::Stdio { command, .. } if command.trim().is_empty() => {
                Err(ConfigError::EmptyCommand(self.name.clone()))
            }
            McpTransport::Http { url, .. }
                if !url.starts_with("http://") && !url.starts_with("https://") =>
            {
                Err(ConfigError::BadUrl(self.name.clone(), url.clone()))
            }
            _ => Ok(()),
        }
    }
}
