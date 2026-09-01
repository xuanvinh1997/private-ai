//! Danh mục server dựng sẵn — cắm bằng một cú bấm.
//!
//! Tồn tại vì bước khó nhất của MCP không phải là chạy một server, mà là **biết gõ gì**.
//! Tên gói đúng, đối số đúng, biến môi trường đúng: ba thứ đó nằm rải trong ba trang tài
//! liệu khác nhau, và gõ sai một trong ba cho ra cùng một kết quả — một server `failed`
//! sau hai mươi giây, không nói vì sao.
//!
//! Hai luật cho bảng dưới đây:
//!
//! **Tên gói phải là tên đang sống.** Nhiều server tham chiếu đã bị bỏ khỏi
//! `@modelcontextprotocol/*` và chuyển sang chỗ khác; một `npx` trỏ vào gói đã ngừng phát
//! hành là một server hỏng mà người dùng không có cách nào đoán ra. Mục nào ở đây cũng đã
//! được tra trên chính sổ đăng ký phát hành gói, không lấy theo trí nhớ.
//!
//! **`requires` là để cảnh báo trước.** Một server stdio cần `node` trên một máy không có
//! `node` không hỏng ngay: nó hỏng sau khi hết thời gian chờ. Hai mươi giây im lặng rồi
//! một chữ "failed" là trải nghiệm tệ nhất có thể; giao diện phải nói được điều đó **trước**
//! khi người dùng bấm.

use std::collections::BTreeMap;

use crate::config::{ConfigError, McpTransport, ServerConfig};

/// Một giá trị người dùng phải điền trước khi cắm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub key: &'static str,
    /// Chữ hiện cạnh ô nhập.
    pub label: &'static str,
    pub required: bool,
    /// Che khi gõ, và đừng ghi ra log. Khoá API và chuỗi kết nối có mật khẩu ở trong.
    pub secret: bool,
}

/// Một server dựng sẵn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Cũng là tên server mặc định, nên nó phải qua được [`ServerConfig::validate`].
    pub id: &'static str,
    pub name: &'static str,
    /// Một câu: nó làm được gì cho người đang đọc.
    pub summary: &'static str,
    pub command: &'static str,
    /// Chỗ nào cần giá trị người dùng thì viết `${TÊN_BIẾN}` — xem [`instantiate`].
    pub args: &'static [&'static str],
    pub env: &'static [EnvVar],
    pub homepage: &'static str,
    /// `node`, `python` hoặc `docker`.
    pub requires: &'static [&'static str],
}

/// Cần `node` để chạy `npx`.
const NODE: &[&str] = &["node"];
/// Cần `python` để chạy `uvx`.
const PYTHON: &[&str] = &["python"];
const DOCKER: &[&str] = &["docker"];

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "filesystem",
        name: "Filesystem",
        summary: "Đọc, ghi và tìm tệp trong những thư mục bạn cho phép, và chỉ những thư mục đó.",
        command: "npx",
        // Thư mục là **đối số dòng lệnh**, không phải biến môi trường: đó là cách server
        // này khai vùng được phép, và cũng là ranh giới an toàn của nó.
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
    },
    CatalogEntry {
        id: "github",
        name: "GitHub",
        summary: "Đọc và viết issue, pull request, mã nguồn và Actions trên GitHub.",
        command: "docker",
        // Bản chính thức của GitHub là một ảnh Docker, không phải một gói npm: gói
        // `@modelcontextprotocol/server-github` cũ đã ngừng phát hành.
        args: &[
            "run",
            "-i",
            "--rm",
            "-e",
            "GITHUB_PERSONAL_ACCESS_TOKEN",
            "ghcr.io/github/github-mcp-server",
        ],
        env: &[EnvVar {
            key: "GITHUB_PERSONAL_ACCESS_TOKEN",
            label: "Personal access token của GitHub",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/github/github-mcp-server",
        requires: DOCKER,
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
    },
    CatalogEntry {
        id: "sqlite",
        name: "SQLite",
        summary: "Truy vấn và sửa một tệp cơ sở dữ liệu SQLite trên máy.",
        command: "uvx",
        args: &["mcp-server-sqlite", "--db-path=${SQLITE_DB_PATH}"],
        env: &[EnvVar {
            key: "SQLITE_DB_PATH",
            label: "Đường dẫn tệp .sqlite",
            required: true,
            secret: false,
        }],
        homepage: "https://pypi.org/project/mcp-server-sqlite/",
        requires: PYTHON,
    },
    CatalogEntry {
        id: "postgres",
        name: "PostgreSQL",
        summary: "Đọc lược đồ, chạy truy vấn và soi hiệu năng trên một cơ sở dữ liệu Postgres.",
        command: "uvx",
        // `restricted` là chế độ chỉ đọc của server này. Mặc định phải là cái hẹp hơn:
        // một chuỗi trong tài liệu người dùng vừa nạp không được biến thành một `DROP`.
        args: &["postgres-mcp", "--access-mode=restricted"],
        env: &[EnvVar {
            key: "DATABASE_URI",
            label: "Chuỗi kết nối, ví dụ postgresql://user:mật-khẩu@localhost:5432/db",
            required: true,
            secret: true,
        }],
        homepage: "https://github.com/crystaldba/postgres-mcp",
        requires: PYTHON,
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
    },
];

/// Tra một mục theo `id`.
pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// Dựng cấu hình từ một mục danh mục cộng những gì người dùng vừa điền.
///
/// Giá trị đi vào một trong hai chỗ, và **không bao giờ cả hai**:
///
/// - Nếu `args` có `${KEY}` thì giá trị được thay vào đó. Một đối số còn sót lại một chỗ
///   trống chưa điền bị **bỏ hẳn** — đó là lý do các đối số tuỳ chọn ở bảng trên viết dạng
///   `--cờ=${KEY}`: bỏ một đối số dính liền thì bỏ cả cờ lẫn giá trị, còn bỏ nửa sau của
///   một cặp rời thì cái cờ trơ lại và server từ chối khởi động.
/// - Ngược lại, giá trị đi vào môi trường của tiến trình con.
///
/// Không nhân đôi vì một bí mật đã nằm trên dòng lệnh thì nó hiện trong `ps` của mọi tiến
/// trình khác; đưa thêm vào môi trường chỉ là thêm một chỗ nữa để nó rò ra.
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
                // Chỗ trống không có gì điền vào: bỏ cả đối số. Đã kiểm ở trên nên chỉ
                // biến **không bắt buộc** rơi vào nhánh này.
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
    config.transport = McpTransport::Stdio {
        command: entry.command.to_string(),
        args,
        env,
        cwd: None,
    };
    // Kiểm ngay tại đây chứ không tin bảng ở trên: bảng là dữ liệu, và một `id` gõ sai lúc
    // thêm mục mới phải hỏng ở bài kiểm chứng chứ không phải ở máy người dùng.
    config.validate()?;
    Ok(config)
}
