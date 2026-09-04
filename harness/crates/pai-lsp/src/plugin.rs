//! Plugs LSP into the tree - and deliberately does not when there is nothing to plug in.
//! Hard rule: if no server is found, no tool is registered, because a tool that always
//! fails teaches the model to ignore the whole tool list. Detection happens once, here.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_tools::Tools;

use crate::config::{LanguageConfig, Limits, defaults};
use crate::launch::{ChildLaunch, Launch, locate};
use crate::pool::{Entry, StdioServers};
use crate::seam::{LanguageServers, Lsp};
use crate::tool::LspTool;

pub struct LspPlugin {
    roots: FileRoots,
    workspace: PathBuf,
    languages: Vec<LanguageConfig>,
    limits: Limits,
}

impl LspPlugin {
    /// `roots` and `protected` should be the same set given to `FsPlugin`; anything else is a way around that boundary.
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
        workspace: PathBuf,
    ) -> LspPlugin {
        LspPlugin {
            roots: FileRoots::new(roots, protected),
            workspace,
            languages: defaults(),
            limits: Limits::default(),
        }
    }

    /// Replace the whole language table - whole block, not merged, as with layered config.
    pub fn with_languages(mut self, languages: Vec<LanguageConfig>) -> LspPlugin {
        self.languages = languages;
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> LspPlugin {
        self.limits = limits;
        self
    }
}

#[async_trait]
impl Plugin for LspPlugin {
    fn name(&self) -> &'static str {
        "lsp"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let mut entries: Vec<Entry> = Vec::new();
        for config in self.languages.iter().filter(|row| row.enabled) {
            let Some(command) = locate(&config.command) else {
                tracing::debug!(
                    language = %config.id, command = %config.command,
                    "not present on this machine; skipping"
                );
                continue;
            };
            let launcher: Arc<dyn Launch> = Arc::new(ChildLaunch::new(
                config.id.clone(),
                command,
                config.args.clone(),
                self.workspace.clone(),
            ));
            entries.push(Entry {
                id: config.id.clone(),
                extensions: config.extensions.clone(),
                launcher,
                options: config.initialization_options.clone(),
            });
        }

        if entries.is_empty() {
            // No provider, no tool, and no error: a machine with no language server installed is normal, not misconfigured.
            tracing::info!("no language server detected; the `lsp` tool is not registered");
            return Ok(());
        }
        tracing::info!(
            languages = ?entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            "language servers detected"
        );

        let servers = Arc::new(StdioServers::new(
            self.workspace.clone(),
            self.roots.clone(),
            entries,
            self.limits,
        ));
        let seam: Arc<dyn LanguageServers> = servers.clone();
        ctx.keep(ctx.provide::<Lsp>(seam.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(LspTool::new(seam))));

        // `shutdown`/`exit` for every running server; async cleanup, so it must be a `defer_async` rather than a `Drop`.
        ctx.effects()
            .defer_async("lsp/servers", move || async move {
                servers.shutdown().await;
            });
        Ok(())
    }
}
