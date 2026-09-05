//! Plugs long-term memory into the tree: one plugin, one seam, four tools.
//!
//! **Application layer, not project layer.** A project-layer plugin (see `pai-rag`'s
//! `RagPlugin`) is unmounted and remounted whenever the user switches project, and its store is
//! keyed by project — which is right for a document library, because the documents *are* the
//! project. Memory is the opposite: what it holds is mostly about the person, not the
//! repository. "Prefers Vietnamese answers", "hates being asked before a refactor", "works with
//! Lan on the payments service" would each have to be relearned in every project, and relearned
//! again in a fresh clone of the same one. So one graph for the whole app, opened once at
//! startup and never swapped.
//!
//! The cost of that choice is that a fact about project A is visible while working on project B.
//! That is acceptable here because this is a single-user desktop app and both projects belong to
//! the same person; it would not be acceptable in a shared deployment. Project scoping, if it is
//! ever wanted, belongs in the graph as an entity kind — not as a second database file.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin, ServiceKey};
use pai_tools::Tools;
use parking_lot::Mutex;

use crate::graph::Graph;
use crate::tools::SharedGraph;
use crate::tools::forget::MemoryForget;
use crate::tools::read::MemoryRead;
use crate::tools::remember::MemoryRemember;
use crate::tools::search::MemorySearch;

/// The graph seam. Exposed so the UI can show what is remembered and let the user delete it —
/// a memory the user cannot inspect is a memory they cannot trust.
pub enum Memory {}
impl ServiceKey for Memory {
    type Api = Mutex<Graph>;
    const NAME: &'static str = "memory.graph";
}

pub struct MemoryPlugin {
    /// The SQLite file. Chosen by the caller rather than derived here, so tests and the app can
    /// point at different places without this crate knowing what a config directory is.
    db_path: PathBuf,
}

impl MemoryPlugin {
    pub fn new(db_path: PathBuf) -> MemoryPlugin {
        MemoryPlugin { db_path }
    }
}

#[async_trait]
impl Plugin for MemoryPlugin {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let graph: SharedGraph = Arc::new(Mutex::new(Graph::open(&self.db_path)?));
        tracing::info!(path = %self.db_path.display(), "long-term memory: native SQLite graph");
        ctx.keep(ctx.provide::<Memory>(graph.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(MemorySearch::new(graph.clone()))));
        ctx.keep(tools.register(Arc::new(MemoryRead::new(graph.clone()))));
        ctx.keep(tools.register(Arc::new(MemoryRemember::new(graph.clone()))));
        ctx.keep(tools.register(Arc::new(MemoryForget::new(graph))));
        Ok(())
    }
}
