//! Cấu hình một server bên thứ ba, và những cái tên nó không được phép mang.
//!
//! Việc kiểm tra ở đây không phải là vệ sinh dữ liệu cho đẹp. Tên server là một **thành
//! phần của danh tính tool** — nó nằm giữa `ext.` và tên tool từ xa — nên một cái tên
//! không kiểm được sẽ đẻ ra một tool không kiểm được. Xem [`ServerConfig::validate`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Bao lâu thì một server không trả lời `initialize` bị coi là không có ở đó.
///
/// Hai mươi giây, lấy từ bản Python. Một server stdio phải kịp `npx` tải gói về lần đầu;
/// dài hơn nữa thì người dùng ngồi nhìn một cửa sổ chưa có tool nào mà không hiểu vì sao.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Thử nối lại bao nhiêu lần trước khi bỏ cuộc.
///
/// Có một giới hạn là bắt buộc: một lệnh gõ sai trong cấu hình mà thử lại vô hạn là một
/// vòng lặp đẻ tiến trình con, và nó chạy im lặng trong nền suốt phiên làm việc.
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
}

/// Đường tới một server. Hai cái, đúng bằng hai cái mà spec định nghĩa.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpTransport {
    /// Một tiến trình con nói JSON-RPC qua stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Thêm vào môi trường của tiến trình con, không thay thế nó.
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Streamable HTTP.
    Http {
        url: String,
        /// Header gửi kèm mọi request — chỗ để một token của người dùng đi vào.
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// Một server bên thứ ba, đúng như người dùng khai nó.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerConfig {
    /// Thành phần giữa của `ext.<name>.<tool>`.
    pub name: String,
    #[serde(flatten)]
    pub transport: McpTransport,
    /// Tắt một server mà không phải xoá cấu hình của nó.
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
    /// Một server stdio với cấu hình mặc định.
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

    /// Một server streamable HTTP với cấu hình mặc định.
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

    /// Kiểm trước khi nối, không phải sau.
    ///
    /// Ba luật về cái tên, và cả ba đều nói về cùng một chuyện — tên server đi vào danh
    /// tính của tool:
    ///
    /// - **Không rỗng**, nếu không thì `ext..search` có hai dấu chấm liền và cái phần lẽ
    ///   ra định danh một server thì không định danh gì cả.
    /// - **Không có dấu chấm**, nếu không thì `a.b` và `a` + tool `b.x` đẻ ra cùng một
    ///   tên đầy đủ từ hai server khác nhau.
    /// - **Không có `__`**, vì `pai-tools` chiếu dấu chấm sang `__` để nói với mô hình, và
    ///   một cái tên chứa sẵn `__` làm phép chiếu đó mất tính khả nghịch. Sổ đăng ký sẽ
    ///   từ chối nó, nhưng từ chối ở đó thì người dùng chỉ thấy tool biến mất; từ chối ở
    ///   đây thì họ đọc được vì sao.
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
