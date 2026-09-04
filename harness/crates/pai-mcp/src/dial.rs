//! Open a connection to a third-party server.
//! Split from [`crate::hub`] so the supervisor loop stays transport-agnostic, and so tests
//! can dial an in-process fake over [`tokio::io::duplex`] with no network and no child.

use std::collections::HashMap;
use std::process::Stdio;

use async_trait::async_trait;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use tokio_util::sync::CancellationToken;

use crate::config::{McpTransport, ServerConfig};

/// How far a connection reaches; used for one decision only, [`pai_tools::ToolMeta::leaves_device`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Same process. Tests only.
    InProcess,
    /// A child process on the same machine.
    ChildProcess,
    /// Over the network.
    Network,
}

impl Reach {
    pub fn leaves_device(self) -> bool {
        matches!(self, Reach::Network)
    }
}

/// How to open a connection; one call means one new connection.
#[async_trait]
pub trait Dialer: Send + Sync + 'static {
    /// `ct` belongs to this connection alone: cancelling it closes the connection, and the supervisor may redial.
    async fn dial(&self, ct: CancellationToken) -> anyhow::Result<RunningService<RoleClient, ()>>;

    /// Defaults to the worst assumption, as [`pai_tools::ToolMeta::default`] does.
    fn reach(&self) -> Reach {
        Reach::Network
    }
}

/// How to build a [`Dialer`] from a config; one thin indirection so hub tests exercise the real `reload` path.
pub trait DialerFactory: Send + Sync + 'static {
    fn make(&self, config: &ServerConfig) -> std::sync::Arc<dyn Dialer>;
}

/// The real one: transport taken straight from the config.
pub struct ConfigDialers;

impl DialerFactory for ConfigDialers {
    fn make(&self, config: &ServerConfig) -> std::sync::Arc<dyn Dialer> {
        std::sync::Arc::new(ConfigDialer::new(config.clone()))
    }
}

/// A [`Dialer`] built from the user's declared config.
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
                // The server's stderr passes through: a broken server usually explains itself there.
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
