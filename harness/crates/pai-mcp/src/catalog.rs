//! A catalogue of ready-made servers, mountable in one click.
//! Two rules for the table: every package name is checked against the live registry, since
//! a dead one just times out, and `requires` lets the UI warn before the user clicks.

use std::collections::BTreeMap;

use crate::config::{ConfigError, McpTransport, ServerConfig};

/// A value the user must supply before mounting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub key: &'static str,
    /// The text shown beside the input.
    pub label: &'static str,
    pub required: bool,
    /// Mask while typing and keep out of logs: API keys and connection strings hold passwords.
    pub secret: bool,
}

/// One ready-made server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Also the default server name, so it has to pass [`ServerConfig::validate`].
    pub id: &'static str,
    pub name: &'static str,
    /// One sentence: what it does for the reader.
    pub summary: &'static str,
    pub command: &'static str,
    /// Write `${VAR_NAME}` wherever a user value belongs — see [`instantiate`].
    pub args: &'static [&'static str],
    pub env: &'static [EnvVar],
    pub homepage: &'static str,
    /// `node`, `python` or `docker`; empty for a remote entry, which needs nothing installed here.
    pub requires: &'static [&'static str],
    /// HTTP endpoint for a remotely hosted server; `Some` makes `command`, `args` and `requires` unused.
    pub url: Option<&'static str>,
}

/// Needs `node` to run `npx`.
const NODE: &[&str] = &["node"];
/// Needs `python` to run `uvx`.
const PYTHON: &[&str] = &["python"];

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "filesystem",
        name: "Filesystem",
        summary: "Đọc, ghi và tìm tệp trong những thư mục bạn cho phép, và chỉ những thư mục đó.",
        command: "npx",
        // The directory is a command-line argument, not an env var: that is how this server declares its allowed area.
        args: &[
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "${FILESYSTEM_ROOT}",
        ],
        env: &[EnvVar {
            key: "FILESYSTEM_ROOT",
            label: "Thư mục được phép truy cập",
            required: true,
            secret: false,
        }],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem",
        requires: NODE,
        url: None,
    },
    CatalogEntry {
        id: "git",
        name: "Git",
        summary: "Đọc lịch sử, xem khác biệt và thao tác trên một kho git ngay trên máy.",
        command: "uvx",
        args: &["mcp-server-git", "--repository=${GIT_REPOSITORY}"],
        env: &[EnvVar {
            key: "GIT_REPOSITORY",
            label: "Đường dẫn kho git (để trống thì mỗi lần gọi tự khai)",
            required: false,
            secret: false,
        }],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/git",
        requires: PYTHON,
        url: None,
    },
    CatalogEntry {
        id: "github",
        name: "GitHub",
        summary: "Đọc và viết issue, pull request, mã nguồn và Actions trên GitHub.",
        // GitHub hosts this server, so there is no child process and `command` is never used.
        command: "",
        args: &[],
        env: &[EnvVar {
            key: "Authorization",
            label: "Personal access token của GitHub (dán nguyên, không cần chữ Bearer)",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/github/github-mcp-server",
        requires: &[],
        url: Some("https://api.githubcopilot.com/mcp/"),
    },
    CatalogEntry {
        id: "fetch",
        name: "Fetch",
        summary: "Tải một trang web về và chuyển sang Markdown để mô hình đọc được.",
        command: "uvx",
        args: &["mcp-server-fetch"],
        env: &[],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch",
        requires: PYTHON,
        url: None,
    },
    CatalogEntry {
        id: "memory",
        name: "Memory",
        summary: "Ghi nhớ sự việc giữa các phiên bằng một đồ thị tri thức trên máy.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-memory"],
        env: &[EnvVar {
            key: "MEMORY_FILE_PATH",
            label: "Tệp lưu trí nhớ (để trống thì dùng mặc định của server)",
            required: false,
            secret: false,
        }],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
        requires: NODE,
        url: None,
    },
    CatalogEntry {
        id: "sequential-thinking",
        name: "Sequential Thinking",
        summary: "Cho mô hình một chỗ để tách bài toán thành từng bước và sửa lại bước đã đi.",
        command: "npx",
        args: &["-y", "@modelcontextprotocol/server-sequential-thinking"],
        env: &[],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
        requires: NODE,
        url: None,
    },
    CatalogEntry {
        id: "time",
        name: "Time",
        summary: "Hỏi giờ hiện tại và đổi giờ giữa các múi giờ.",
        command: "uvx",
        args: &["mcp-server-time"],
        env: &[],
        homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/time",
        requires: PYTHON,
        url: None,
    },
    CatalogEntry {
        id: "postgres",
        name: "PostgreSQL",
        summary: "Đọc lược đồ, chạy truy vấn và soi hiệu năng trên một cơ sở dữ liệu Postgres.",
        command: "uvx",
        // `restricted` is this server's read-only mode; a line in a freshly loaded document must not become a `DROP`.
        args: &["postgres-mcp", "--access-mode=restricted"],
        env: &[EnvVar {
            key: "DATABASE_URI",
            label: "Chuỗi kết nối, ví dụ postgresql://user:mật-khẩu@localhost:5432/db",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/crystaldba/postgres-mcp",
        requires: PYTHON,
        url: None,
    },
    CatalogEntry {
        id: "playwright",
        name: "Playwright",
        summary: "Điều khiển một trình duyệt thật: mở trang, bấm, điền biểu mẫu, đọc nội dung.",
        command: "npx",
        args: &["-y", "@playwright/mcp@latest"],
        env: &[],
        homepage: "https://github.com/microsoft/playwright-mcp",
        requires: NODE,
        url: None,
    },
    CatalogEntry {
        id: "brave-search",
        name: "Brave Search",
        summary: "Tìm trên web, tin tức, ảnh và địa điểm qua API của Brave.",
        command: "npx",
        args: &[
            "-y",
            "@brave/brave-search-mcp-server",
            "--transport",
            "stdio",
        ],
        env: &[EnvVar {
            key: "BRAVE_API_KEY",
            label: "Khoá API của Brave Search",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/brave/brave-search-mcp-server",
        requires: NODE,
        url: None,
    },
    CatalogEntry {
        id: "slack",
        name: "Slack",
        summary: "Đọc kênh, luồng và tin nhắn riêng trong một workspace Slack.",
        command: "npx",
        args: &["-y", "slack-mcp-server@latest", "--transport", "stdio"],
        env: &[EnvVar {
            key: "SLACK_MCP_XOXP_TOKEN",
            label: "Token người dùng Slack (xoxp-…)",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/korotovsky/slack-mcp-server",
        requires: NODE,
        url: None,
    },
];

/// Look up an entry by `id`.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// Build a config from a catalogue entry plus the user's values, which go either into an `${KEY}` slot or the child's
/// environment, never both; an argument with an unfilled slot is dropped whole, hence the `--flag=${KEY}` form.
pub fn instantiate(
    entry: &CatalogEntry,
    values: &BTreeMap<String, String>,
) -> Result<ServerConfig, ConfigError> {
    let filled = |key: &str| -> Option<&str> {
        values
            .get(key)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    };

    let missing: Vec<&str> = entry
        .env
        .iter()
        .filter(|var| var.required && filled(var.key).is_none())
        .map(|var| var.key)
        .collect();
    if !missing.is_empty() {
        return Err(ConfigError::MissingValue(
            entry.id.to_string(),
            missing.join(", "),
        ));
    }

    let mut inline: Vec<&str> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    'outer: for raw in entry.args {
        let mut arg = (*raw).to_string();
        for var in entry.env {
            let slot = format!("${{{}}}", var.key);
            if !arg.contains(&slot) {
                continue;
            }
            let Some(value) = filled(var.key) else {
                // Nothing to fill the slot, so drop the argument; the check above leaves only optional vars here.
                continue 'outer;
            };
            arg = arg.replace(&slot, value);
            inline.push(var.key);
        }
        args.push(arg);
    }

    let env: BTreeMap<String, String> = entry
        .env
        .iter()
        .filter(|var| !inline.contains(&var.key))
        .filter_map(|var| Some((var.key.to_string(), filled(var.key)?.to_string())))
        .collect();

    let mut config = ServerConfig::stdio(entry.id, entry.command);
    config.transport = match entry.url {
        // Remote: user values become headers, not env vars, which would never reach the far server and fail as a silent 401.
        Some(template) => {
            let mut url = template.to_string();
            for var in entry.env {
                let slot = format!("${{{}}}", var.key);
                if let Some(value) = filled(var.key) {
                    url = url.replace(&slot, value);
                }
            }
            McpTransport::Http {
                url,
                headers: env.into_iter().collect(),
            }
        }
        None => McpTransport::Stdio {
            command: entry.command.to_string(),
            args,
            env,
            cwd: None,
        },
    };
    // Validate here rather than trusting the table: a mistyped `id` must fail in tests, not on a user's machine.
    config.validate()?;
    Ok(config)
}
