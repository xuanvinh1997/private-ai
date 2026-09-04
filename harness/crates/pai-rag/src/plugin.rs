//! Plugs the document library into the tree: one plugin, one provider, three read tools.
//! Project-layer plugin, so switching projects unplugs and replugs it with a new root.
//! The four management tools stay unregistered — only a human action may reach them.
//!
//! Mounted twice over the application, never at once: over the user's document folder in a document project,
//! and over a code project's attachment folder, where the same extractors turn an attached PDF, image or DOCX
//! into something the model can read. Two plugin names for one plugin, since the tree keys scopes by name.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_asr::Asr;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::library::{DocLibrary, Docs};
use crate::native::NativeLibrary;
use crate::tools::list::DocsList;
use crate::tools::read::DocsRead;
use crate::tools::search::DocsSearch;
use crate::tools::{ATTACHMENTS, DOCS, Vocab};

pub struct RagPlugin {
    /// Which mount this is; returned by [`Plugin::name`], which the tree uses to key the scope.
    name: &'static str,
    /// What the three tools are called at this mount.
    ten: Vocab,
    config_path: PathBuf,
    project: String,
    /// The folder the library reads. The library *is* that folder; nothing is copied.
    root: PathBuf,
    /// The app's one speech recognizer, so an audio file in a project and the microphone in the
    /// composer share a single loaded model.
    asr: Asr,
}

impl RagPlugin {
    /// The document project's own library: the folder the user chose.
    pub fn new(config_path: PathBuf, project: String, root: PathBuf, asr: Asr) -> RagPlugin {
        RagPlugin {
            name: "rag",
            ten: DOCS,
            config_path,
            project,
            root,
            asr,
        }
    }

    /// The same library over a code project's attachment folder. A separate `project` id, so its store and its
    /// vector collection never share a name with a document library.
    pub fn attachments(config_path: PathBuf, project: String, root: PathBuf, asr: Asr) -> RagPlugin {
        RagPlugin {
            name: "attachments",
            ten: ATTACHMENTS,
            ..RagPlugin::new(config_path, project, root, asr)
        }
    }
}

#[async_trait]
impl Plugin for RagPlugin {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let docs: Arc<dyn DocLibrary> = Arc::new(NativeLibrary::open(
            self.config_path.clone(),
            self.project.clone(),
            self.root.clone(),
            self.asr.clone(),
        )?);
        tracing::info!("document library backend: native Rust");
        ctx.keep(ctx.provide::<Docs>(docs.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(DocsSearch::new(docs.clone(), self.ten))));
        ctx.keep(tools.register(Arc::new(DocsRead::new(docs.clone(), self.ten))));
        ctx.keep(tools.register(Arc::new(DocsList::new(docs, self.ten))));
        Ok(())
    }
}
