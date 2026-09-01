//! Mở một kết nối tới một server bên thứ ba.
//!
//! Tách ra khỏi [`crate::hub`] vì hai lý do khác nhau và cả hai đều đáng:
//!
//! - **Giám sát không cần biết transport.** Vòng lặp nối lại, đếm số lần thử, đăng ký và
//!   gỡ tool giống hệt nhau cho stdio và cho HTTP. Trộn chúng vào nhau là viết vòng lặp
//!   đó hai lần rồi để hai bản trôi ra khỏi nhau.
//! - **Bài kiểm chứng nối được vào đây.** Một [`Dialer`] dựng sẵn một server giả trong
//!   tiến trình qua [`tokio::io::duplex`] cho phép kiểm bất biến tiền tố, bất biến
//!   best-effort và hot-reload mà không đụng mạng, không đẻ tiến trình con, và không phụ
//!   thuộc vào một `npx` có trên máy chạy CI hay không.

use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use tokio_util::sync::CancellationToken;

use crate::config::{McpTransport, ServerConfig};

/// Kết nối đi xa tới đâu.
///
/// Chỉ dùng cho đúng một quyết định: [`pai_tools::ToolMeta::leaves_device`]. Một server
/// stdio là một tiến trình khác trên cùng máy — đáng nghi, nhưng dữ liệu chưa rời máy;
/// một server HTTP thì có. Phân biệt được hai chuyện đó thì cảnh báo "rời máy" mới còn
/// nghĩa; gộp lại thì nó kêu ở mọi lần gọi và người dùng học cách bỏ qua nó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Cùng tiến trình. Chỉ có trong bài kiểm chứng.
    InProcess,
    /// Tiến trình con trên cùng máy.
    ChildProcess,
    /// Qua mạng.
    Network,
}

impl Reach {
    pub fn leaves_device(self) -> bool {
        matches!(self, Reach::Network)
    }
}

/// Cách mở một kết nối. Một lần gọi = một kết nối mới.
#[async_trait]
pub trait Dialer: Send + Sync + 'static {
    /// `ct` là token của **riêng kết nối này**: huỷ nó là đóng kết nối, và task giám sát
    /// vẫn được phép nối lại bằng một token con mới.
    async fn dial(&self, ct: CancellationToken) -> anyhow::Result<RunningService<RoleClient, ()>>;

    /// Mặc định là giả định xấu nhất, cùng tinh thần với [`pai_tools::ToolMeta::default`].
    fn reach(&self) -> Reach {
        Reach::Network
    }
}

/// [`Dialer`] dựng từ cấu hình người dùng khai.
pub struct ConfigDialer {
    config: ServerConfig,
}

impl ConfigDialer {
    pub fn new(config: ServerConfig) -> ConfigDialer {
        ConfigDialer { config }
    }
}

#[async_trait]
impl Dialer for ConfigDialer {
    async fn dial(&self, ct: CancellationToken) -> anyhow::Result<RunningService<RoleClient, ()>> {
        match &self.config.transport {
            McpTransport::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args);
                for (key, value) in env {
                    cmd.env(key, value);
                }
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                // stderr của server đi thẳng ra stderr của ta: một server hỏng thường nói
                // lý do ở đó, và nuốt nó đi là biến "không nối được" thành một bí ẩn.
                let (transport, _) = TokioChildProcess::builder(cmd)
                    .stderr(Stdio::inherit())
                    .spawn()?;
                Ok(().serve_with_ct(transport, ct).await?)
            }
            McpTransport::Http { url, headers } => {
                let mut custom = HashMap::new();
                for (key, value) in headers {
                    let name = http::HeaderName::from_bytes(key.as_bytes())
                        .map_err(|err| anyhow::anyhow!("header `{key}` không hợp lệ: {err}"))?;
                    let value = http::HeaderValue::from_str(value).map_err(|err| {
                        anyhow::anyhow!("giá trị header `{key}` không hợp lệ: {err}")
                    })?;
                    custom.insert(name, value);
                }
                let transport = StreamableHttpClientTransport::from_config(
                    StreamableHttpClientTransportConfig::with_uri(url.clone())
                        .custom_headers(custom),
                );
                Ok(().serve_with_ct(transport, ct).await?)
            }
        }
    }

    fn reach(&self) -> Reach {
        match self.config.transport {
            McpTransport::Stdio { .. } => Reach::ChildProcess,
            McpTransport::Http { .. } => Reach::Network,
        }
    }
}
