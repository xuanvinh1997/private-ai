//! Cắm MCP vào cây.
//!
//! Một plugin cho cả hai chiều, vì cả hai chiều đứng và ngã cùng nhau: gỡ nó ra là đóng
//! mọi kết nối ra ngoài *và* đóng cái cổng vào trong. Tách làm hai plugin thì có một trạng
//! thái mà không ai muốn — cổng còn mở trong khi client đã tắt, hoặc ngược lại.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::{ToolPipeline, Tools};
use tokio_util::sync::CancellationToken;

use crate::config::ServerConfig;
use crate::expose::RegistryServer;
use crate::hub::{McpHub, Mount};
use crate::seam::{Mcp, McpConfig};
use crate::serve::{serve_http, serve_stdio};
use crate::store::{McpStore, apply, merge};
use crate::token::{McpToken, token_path};

/// Phơi sổ đăng ký ra ngoài ở đâu.
#[derive(Clone, Debug)]
pub struct ExposeOptions {
    /// Nơi đặt `mcp-token`. **Phải** được thêm vào danh sách đường dẫn được bảo vệ của
    /// `pai-fs` — xem [`crate::token`].
    pub data_dir: PathBuf,
    /// Nói MCP trên stdin/stdout của chính tiến trình này.
    pub stdio: bool,
    /// Địa chỉ HTTP. Phải là loopback, nếu không [`serve_http`] từ chối.
    pub http: Option<SocketAddr>,
    /// `Origin` được chấp nhận. Rỗng = từ chối mọi request có mang `Origin`.
    pub allowed_origins: Vec<String>,
}

impl ExposeOptions {
    pub fn new(data_dir: impl Into<PathBuf>) -> ExposeOptions {
        ExposeOptions {
            data_dir: data_dir.into(),
            stdio: false,
            http: None,
            allowed_origins: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct McpPlugin {
    servers: Vec<ServerConfig>,
    store: Option<PathBuf>,
    expose: Option<ExposeOptions>,
}

impl McpPlugin {
    pub fn new(servers: Vec<ServerConfig>) -> McpPlugin {
        McpPlugin {
            servers,
            store: None,
            expose: None,
        }
    }

    /// Chỗ đặt kho server người dùng tự quản.
    ///
    /// Tuỳ chọn, và phải tuỳ chọn: một `patch.yaml` đang khai sẵn server phải chạy y như
    /// trước dù chưa ai bật màn hình quản lý MCP lên bao giờ. Có kho thì kho được **hợp
    /// nhất** với hàng cấu hình — xem [`merge`] — chứ không thay thế nó.
    pub fn storing(mut self, path: impl Into<PathBuf>) -> McpPlugin {
        self.store = Some(path.into());
        self
    }

    pub fn exposing(mut self, options: ExposeOptions) -> McpPlugin {
        self.expose = Some(options);
        self
    }
}

#[async_trait]
impl Plugin for McpPlugin {
    fn name(&self) -> &'static str {
        "mcp"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let registry = ctx.require::<Tools>()?;
        let hub = McpHub::new(registry.clone());
        ctx.keep(ctx.provide::<Mcp>(hub.clone())?);
        {
            let hub = hub.clone();
            ctx.effects()
                .defer_async("mcp/hub", move || async move { hub.shutdown().await });
        }

        // Kho được cắm làm seam trước khi nối bất cứ thứ gì: màn hình quản lý MCP phải mở
        // ra được ngay cả khi mọi server đều đang hỏng, và danh sách người dùng khai không
        // phụ thuộc vào việc có nối được hay không.
        let store = match &self.store {
            Some(path) => {
                let store = Arc::new(McpStore::open(path.clone()));
                ctx.keep(ctx.provide::<McpConfig>(store.clone())?);
                Some(store)
            }
            None => None,
        };

        // Nối server bên thứ ba **trong nền**. Mỗi cái được phép ngốn tới hai mươi giây
        // trước khi bị coi là không có ở đó, và cộng dồn lại thì cửa sổ ứng dụng đứng im
        // chờ những server không phải của ta — đúng cái mà "best-effort" tồn tại để tránh.
        //
        // Lượt khởi động đi qua đúng đường mà mọi thay đổi sau này đi: [`apply`], tức là
        // [`McpHub::reload`]. Một đường riêng cho lượt đầu là một đường không ai chạy lại,
        // và nó sẽ trôi ra khỏi đường kia mà không ai nhận ra.
        let rows = self.servers.clone();
        let hub_for_mount = hub.clone();
        tokio::spawn(async move {
            let report = match &store {
                Some(store) => match apply(&hub_for_mount, store, &rows).await {
                    Ok(report) => report,
                    Err(err) => {
                        // Kho hỏng không được kéo theo hàng cấu hình: người dùng vẫn phải
                        // có những server mà bản cài đặt mồi sẵn cho họ.
                        tracing::warn!(%err, "không đọc được kho MCP, chỉ dùng hàng cấu hình");
                        hub_for_mount.reload(merge(rows, Vec::new())).await
                    }
                },
                None => hub_for_mount.reload(rows).await,
            };
            for (name, result) in report {
                match result {
                    Ok(Mount::Connected { tools }) => {
                        tracing::info!(server = %name, tools, "đã cắm MCP server");
                    }
                    Ok(Mount::Unavailable { reason }) => {
                        tracing::warn!(server = %name, %reason, "MCP server chưa dùng được");
                    }
                    Err(err) => tracing::warn!(server = %name, %err, "cấu hình MCP server sai"),
                }
            }
        });

        let Some(options) = self.expose.clone() else {
            return Ok(());
        };

        // Đường ống dựng từ `ctx` của plugin, nên phạm vi của nó là phạm vi gốc: một
        // client bên ngoài không phải một agent con và không thừa hưởng hạn chế của ai.
        let pipeline = Arc::new(ToolPipeline::new(ctx, registry));
        let token = McpToken::load_or_create(&token_path(&options.data_dir))?;

        if let Some(bind) = options.http {
            let ct = CancellationToken::new();
            let endpoint = serve_http(
                RegistryServer::new(pipeline.clone()),
                bind,
                token,
                options.allowed_origins.clone(),
                ct,
            )
            .await?;
            ctx.effects()
                .defer_async("mcp/http", move || async move { endpoint.shutdown().await });
        }

        if options.stdio {
            let ct = CancellationToken::new();
            let server = RegistryServer::new(pipeline);
            let stop = ct.clone();
            let handle = tokio::spawn(async move {
                if let Err(err) = serve_stdio(server, ct).await {
                    tracing::warn!(%err, "MCP stdio dừng");
                }
            });
            ctx.effects().defer_async("mcp/stdio", move || async move {
                stop.cancel();
                let _ = handle.await;
            });
        }

        Ok(())
    }
}
