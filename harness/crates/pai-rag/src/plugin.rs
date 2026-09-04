//! Plugs the document library into the tree: one plugin, one provider, three read tools.
//! Project-layer plugin, so switching projects unplugs and replugs it with a new root.
//! The four management tools stay unregistered — only a human action may reach them.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::client::RagClient;
use crate::library::{DocLibrary, Docs};
use crate::sidecar::{Sidecar, SidecarConfig};
use crate::tools::list::DocsList;
use crate::tools::read::DocsRead;
use crate::tools::search::DocsSearch;

pub struct RagPlugin {
    config: SidecarConfig,
    /// The user's document folder. The library *is* that folder; nothing is copied.
    root: PathBuf,
}

impl RagPlugin {
    pub fn new(config: SidecarConfig, root: PathBuf) -> RagPlugin {
        RagPlugin { config, root }
    }
}

#[async_trait]
impl Plugin for RagPlugin {
    fn name(&self) -> &'static str {
        "rag"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        // Do not connect here: the connection opens on the first call, so opening a project does not pay the Python startup cost.
        let sidecar = Arc::new(Sidecar::new(self.config.clone()));
        let client = Arc::new(RagClient::new(sidecar, self.root.clone()));

        // Close the child on teardown, or switching projects leaks a Python process per switch.
        let closing = client.clone();
        ctx.effects().defer("rag/shutdown", move || {
            let closing = closing.clone();
            tokio::spawn(async move { closing.shutdown().await });
        });

        let docs: Arc<dyn DocLibrary> = client;
        ctx.keep(ctx.provide::<Docs>(docs.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(DocsSearch::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsRead::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsList::new(docs))));
        Ok(())
    }
}
