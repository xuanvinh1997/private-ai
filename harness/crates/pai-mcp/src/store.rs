//! Kho cấu hình server: **một tệp JSON**, không phải một cơ sở dữ liệu.
//!
//! Mọi kho khác trong repo này là SQLite, nên chỗ này phải nói ra vì sao nó khác. Người
//! dùng không gõ cấu hình MCP từ đầu — họ **dán** nó từ tài liệu của bên thứ ba, và mọi
//! tài liệu ngoài kia đều viết cùng một khối `{"mcpServers": {...}}`. Một kho mở ra dán
//! vào được, và mở ra sửa được khi ứng dụng không chịu chạy, là kho đúng cho thứ này; một
//! bảng SQLite biến thao tác mười giây đó thành một thứ chỉ ứng dụng chạm được.
//!
//! Vì thế kho đọc **cả hai** hình dạng: hình dạng của Claude Desktop / codex, và hình dạng
//! gốc của crate này (`{"servers": [...]}`). Nhưng chỉ **ghi ra một** — dạng `mcpServers`,
//! vì đó là dạng người dùng sẽ đọc lại và so với tài liệu họ vừa dán.
//!
//! Tệp này chứa token: `env` của một server stdio và `headers` của một server HTTP là đúng
//! chỗ khoá API đi vào. Nó được ghi `0600`, và được ghi **nguyên tử**.

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

/// Danh sách server người dùng tự quản, trên đĩa.
pub struct McpStore {
    path: PathBuf,
    /// Mỗi lần ghi là một chu trình đọc → sửa → ghi. Hai lời gọi song song mà không có
    /// khoá này thì cái ghi sau dựng lại từ ảnh chụp cũ và **nuốt mất** cái ghi trước —
    /// người dùng bấm thêm hai server rồi thấy còn một.
    writing: Mutex<()>,
}

impl McpStore {
    pub fn open(path: PathBuf) -> McpStore {
        McpStore {
            path,
            writing: Mutex::new(()),
        }
    }

    /// Đường dẫn của tệp, để giao diện chỉ cho người dùng chỗ mở ra sửa tay.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Toàn bộ server đã lưu, kể cả cái đang tắt.
    ///
    /// Chưa có tệp = chưa có server nào, không phải lỗi: lần chạy đầu tiên nào cũng rơi
    /// vào đúng trạng thái đó.
    pub fn list(&self) -> Result<Vec<ServerConfig>, StoreError> {
        let Some(text) = self.read()? else {
            return Ok(Vec::new());
        };
        let shape: FileShape = serde_json::from_str(&text)
            .map_err(|err| StoreError::Malformed(self.path.clone(), err))?;
        Ok(shape.into_configs())
    }

    /// Thêm hoặc thay một server. Trùng tên là **thay thế**, vì tên là danh tính.
    ///
    /// Kiểm ngay ở cửa vào chứ không đợi lúc nối: một cấu hình hỏng đã nằm trong tệp thì
    /// nó hỏng ở mọi lần khởi động sau, và chỗ báo lỗi lúc đó cách xa chỗ người dùng vừa
    /// gõ sai đúng một phiên làm việc.
    pub fn save(&self, config: ServerConfig) -> Result<(), StoreError> {
        config.validate()?;
        let _writing = self.writing.lock();
        let mut configs = self.list()?;
        configs.retain(|existing| existing.name != config.name);
        configs.push(config);
        self.write(&configs)
    }

    /// `false` nghĩa là không có gì để xoá — không phải lỗi: hai cú bấm xoá liên tiếp trên
    /// cùng một hàng phải cho cùng một kết quả.
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

    /// Bật/tắt mà **không** xoá cấu hình: một server tắt vẫn giữ nguyên token và tham số
    /// của nó, nên bật lại là một cú bấm chứ không phải một lần dán lại khoá API.
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

    /// Ghi nguyên tử: một tệp tạm cùng thư mục, rồi `rename`.
    ///
    /// Không phải sự cẩn thận thừa. Ghi đè thẳng lên tệp cũ mà mất điện giữa chừng để lại
    /// một tệp JSON cụt, và lần khởi động sau **toàn bộ** server ngoài của người dùng biến
    /// mất cùng lúc — không có thông báo nào, chỉ là một danh sách tool ngắn đi. `rename`
    /// trong cùng một thư mục là nguyên tử, nên tệp cũ đứng nguyên cho tới khi tệp mới đã
    /// nằm trọn trên đĩa.
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
                // Một tệp tạm bỏ lại là rác trong thư mục dữ liệu của người dùng, và ở
                // đây nó là rác **có token bên trong**.
                let _ = fs::remove_file(&tmp);
                Err(err)
            }
        }
    }

    fn spill(&self, tmp: &Path, body: &[u8]) -> Result<(), StoreError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // `0600` đặt **lúc tạo**, không phải sau khi ghi: giữa hai thao tác đó tệp tạm đã
        // chứa token và đã đọc được bởi mọi tài khoản trên máy.
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
        // Đẩy xuống đĩa trước khi đổi tên. Không có bước này thì `rename` chỉ nguyên tử
        // với thứ tự thao tác, chứ không với nội dung: tên mới trỏ vào một tệp mà dữ liệu
        // còn nằm trong bộ đệm.
        file.sync_all()
            .map_err(|err| StoreError::Io(tmp.to_path_buf(), err))?;
        drop(file);
        fs::rename(tmp, &self.path).map_err(|err| StoreError::Io(self.path.clone(), err))
    }
}

/// Tên tệp tạm phải khác nhau giữa hai tiến trình cùng ghi, nếu không `create_new` của
/// tiến trình thứ hai hỏng và người dùng thấy một lần lưu thất bại không lý do.
fn temp_name(path: &Path) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "mcp.json".to_string());
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(".{base}.{}.{seq}.tmp", std::process::id())
}

/// Hợp nhất hai nguồn cấu hình: hàng `mcp` trong tệp vá, và kho người dùng tự quản.
///
/// Kho **thắng** khi trùng tên. Hàng cấu hình là thứ bản cài đặt mồi sẵn hoặc quản trị
/// viên đặt vào; kho là thứ người dùng vừa bấm trong ứng dụng ba giây trước. Cho hàng cấu
/// hình thắng nghĩa là một cú bấm "tắt" im lặng không có tác dụng, và đó là loại lỗi người
/// dùng không báo cáo được vì họ tưởng mình bấm hụt.
pub fn merge(rows: Vec<ServerConfig>, stored: Vec<ServerConfig>) -> Vec<ServerConfig> {
    let mut by_name: BTreeMap<String, ServerConfig> = BTreeMap::new();
    for config in rows.into_iter().chain(stored) {
        by_name.insert(config.name.clone(), config);
    }
    by_name.into_values().collect()
}

/// **Đường duy nhất** đưa cấu hình lên một hub đang chạy.
///
/// Một chỗ chứ không phải mỗi lệnh một chỗ, vì thêm/xoá/bật/tắt phải đi qua đúng cùng phép
/// so sánh của [`McpHub::reload`] — thứ giữ cho một thao tác trên server A không đụng tới
/// kết nối đang khoẻ của server B. Ai đó tự viết một đường tắt "gỡ hết rồi cắm lại" sẽ
/// đúng ở mọi bài kiểm chứng và sai ở mọi lần dùng thật.
pub async fn apply(
    hub: &McpHub,
    store: &McpStore,
    rows: &[ServerConfig],
) -> Result<Vec<(String, Result<Mount, ConfigError>)>, StoreError> {
    let configs = merge(rows.to_vec(), store.list()?);
    Ok(hub.reload(configs).await)
}

/// Hình dạng của tệp trên đĩa — cả hai kiểu, trong một struct.
///
/// Một struct chứ không phải một `enum` untagged, vì một tệp có **cả hai** khối không phải
/// chuyện lạ: người dùng dán thêm một khối `mcpServers` vào cạnh cái đã có. Từ chối cả tệp
/// vì lý do đó là mất hết server chỉ vì một phần thừa.
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
                // Bỏ đúng một mục chứ không bỏ cả tệp: một mục dán thiếu `command` là lỗi
                // của một dòng, và những server còn lại của người dùng không liên quan.
                None => tracing::warn!(
                    server = %name,
                    "bỏ qua mục MCP trong kho: không có `command` lẫn `url` để đi tới đâu"
                ),
            }
        }
        // Hình dạng gốc ghi đè hình dạng dán vào: nó nói được nhiều hơn (transport tường
        // minh, số lần thử lại), nên khi hai khối cùng khai một cái tên thì cái nói rõ hơn
        // là cái người viết tệp có chủ ý hơn.
        for config in self.servers {
            by_name.insert(config.name.clone(), config);
        }
        by_name.into_values().collect()
    }
}

/// Một mục trong khối `mcpServers`, đọc rộng hơn những gì ta ghi ra.
///
/// Không `deny_unknown_fields`: cấu hình người dùng dán vào thường mang theo `description`,
/// `type`, hay một khoá riêng của công cụ khác. Từ chối vì một khoá thừa là bắt người dùng
/// dọn dẹp một tệp mà họ chỉ muốn dán.
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
    /// Vài công cụ khác nói cùng một chuyện bằng từ ngược lại. Đọc cả hai, ghi ra một —
    /// nếu không, một tệp dán từ chỗ khác sẽ hiện lên là đang bật trong khi người dùng đã
    /// tắt nó ở nơi họ chép sang.
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

    /// `None` nghĩa là mục này không nói được nó đi tới đâu.
    fn into_config(self, name: &str) -> Option<ServerConfig> {
        // `url` xét trước: một mục có cả hai là một mục dán chồng lên nhau, và địa chỉ
        // mạng là thứ cụ thể hơn một cái lệnh còn sót lại.
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
        // Dựng từ hàm khởi tạo có sẵn rồi thay transport, để mọi mặc định (thời gian chờ,
        // số lần thử lại) đến từ đúng một chỗ trong [`crate::config`].
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
