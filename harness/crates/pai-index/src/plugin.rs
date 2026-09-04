//! Mount the index into the tree.
//! One plugin, one provider, five tools. Removing it loses `symbol_search`, `outline` and
//! the three graph tools and nothing else: no other crate's tools call into the index.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_tools::Tools;

use crate::index::{CodeIndex, Index, SymbolIndex};
use crate::tools::graph::CodeGraph;
use crate::tools::outline::Outline;
use crate::tools::overview::CodeOverview;
use crate::tools::symbol_search::SymbolSearch;
use crate::tools::trace::CodeTrace;

pub struct IndexPlugin {
    roots: FileRoots,
    /// The directory holding the index file, not the file itself — see [`db_name`].
    dir: PathBuf,
}

impl IndexPlugin {
    /// `roots` and `protected` should be the same set given to `FsPlugin`: an index that sees more is a way around it.
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
        dir: PathBuf,
    ) -> IndexPlugin {
        IndexPlugin {
            roots: FileRoots::new(roots, protected),
            dir,
        }
    }
}

/// The index filename for a working directory, derived rather than passed in: a caller-supplied path would let two workspaces share one index.
fn db_name(root: &std::path::Path) -> String {
    let text = root.display().to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let label: String = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into())
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    let label = if label.is_empty() {
        "root".to_string()
    } else {
        label
    };
    format!("{label}-{hash:016x}.sqlite")
}

#[async_trait]
impl Plugin for IndexPlugin {
    fn name(&self) -> &'static str {
        "index"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let root = self
            .roots
            .roots()
            .first()
            .ok_or_else(|| anyhow::anyhow!("chỉ mục cần ít nhất một thư mục được cấp quyền"))?;
        let db = self.dir.join(db_name(root));
        let index = Arc::new(CodeIndex::open(self.roots.clone(), &db)?);
        let paths = index.refresh_paths().await?;
        let service: Arc<dyn SymbolIndex> = index.clone();
        ctx.keep(ctx.provide::<Index>(service.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(SymbolSearch::new(service.clone()))));
        ctx.keep(tools.register(Arc::new(CodeGraph::new(service.clone()))));
        ctx.keep(tools.register(Arc::new(CodeTrace::new(service.clone()))));
        ctx.keep(tools.register(Arc::new(CodeOverview::new(service.clone()))));
        ctx.keep(tools.register(Arc::new(Outline::new(service, self.roots.clone()))));

        // Completion is ready after the cheap walk above; parsing and edge resolution warm in the background.
        tokio::spawn(async move {
            match index.sync().await {
                Ok(report) => tracing::debug!(paths, ?report, "warmed the code index"),
                Err(err) => tracing::warn!(error = %err, "could not warm the code index"),
            }
        });
        Ok(())
    }
}
