//! Mount MCP into the tree.
//! One plugin for both directions, because they stand and fall together: two plugins would
//! allow the gate to stay open after the client side is gone, or the reverse.

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

/// Where the registry gets exposed.
#[derive(Clone, Debug)]
pub struct ExposeOptions {
    /// Where `mcp-token` lives; must be in `pai-fs`'s protected paths — see [`crate::token`].
    pub data_dir: PathBuf,
    /// Speak MCP over this process's own stdin/stdout.
    pub stdio: bool,
    /// HTTP address; must be loopback or [`serve_http`] refuses.
    pub http: Option<SocketAddr>,
    /// Accepted `Origin`s; empty rejects every request that carries one.
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

    /// Where the user-managed server store lives; optional, and merged with the config rows rather than replacing them.
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

        // The store is provided before anything dials: the MCP screen must open even when every server is broken.
        let store = match &self.store {
            Some(path) => {
                let store = Arc::new(McpStore::open(path.clone()));
                ctx.keep(ctx.provide::<McpConfig>(store.clone())?);
                Some(store)
            }
            None => None,
        };

        // Dial in the background, through the same [`apply`] path every later change takes, so startup never blocks the window.
        let rows = self.servers.clone();
        let hub_for_mount = hub.clone();
        tokio::spawn(async move {
            let report = match &store {
                Some(store) => match apply(&hub_for_mount, store, &rows).await {
                    Ok(report) => report,
                    Err(err) => {
                        // A broken store must not take the config rows with it.
                        tracing::warn!(%err, "could not read the MCP store, using config rows only");
                        hub_for_mount.reload(merge(rows, Vec::new())).await
                    }
                },
                None => hub_for_mount.reload(rows).await,
            };
            for (name, result) in report {
                match result {
                    Ok(Mount::Connected { tools }) => {
                        tracing::info!(server = %name, tools, "mounted MCP server");
                    }
                    Ok(Mount::Unavailable { reason }) => {
                        tracing::warn!(server = %name, %reason, "MCP server not usable yet");
                    }
                    Err(err) => tracing::warn!(server = %name, %err, "invalid MCP server config"),
                }
            }
        });

        let Some(options) = self.expose.clone() else {
            return Ok(());
        };

        // The pipeline is built from the plugin's `ctx`, so its scope is the root: an outside client inherits no limits.
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
                    tracing::warn!(%err, "MCP stdio stopped");
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
