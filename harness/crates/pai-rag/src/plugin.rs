//! Plugs the document library into the tree: one plugin, one provider, three read tools.
//! Project-layer plugin, so switching projects unplugs and replugs it with a new root.
//! The four management tools stay unregistered — only a human action may reach them.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::library::{DocLibrary, Docs};
use crate::native::NativeLibrary;
use crate::tools::list::DocsList;
use crate::tools::read::DocsRead;
use crate::tools::search::DocsSearch;

pub struct RagPlugin {
    config_path: PathBuf,
    project: String,
    /// The user's document folder. The library *is* that folder; nothing is copied.
    root: PathBuf,
}

impl RagPlugin {
    pub fn new(config_path: PathBuf, project: String, root: PathBuf) -> RagPlugin {
        RagPlugin {
            config_path,
            project,
            root,
        }
    }
}

#[async_trait]
impl Plugin for RagPlugin {
    fn name(&self) -> &'static str {
        "rag"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let docs: Arc<dyn DocLibrary> = Arc::new(NativeLibrary::open(
            self.config_path.clone(),
            self.project.clone(),
            self.root.clone(),
        )?);
        tracing::info!("document library backend: native Rust");
        ctx.keep(ctx.provide::<Docs>(docs.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(DocsSearch::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsRead::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsList::new(docs))));
        Ok(())
    }
}
