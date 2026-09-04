//! Mount the filesystem into the tree: one plugin, six tools, one provider, one policy.
//! Disposing it drops the tools and the read-before-edit rule together, as it should.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::{Overflow, Tools};

use crate::observed::{ReadBeforeEdit, ReadLedger};
use crate::path::FileRoots;
use crate::provider::{Fs, FsProvider, LocalFs};
use crate::tools::{
    edit::Edit, glob::GlobTool, grep::Grep, list::ListDir, read::Read, write::Write,
};

pub struct FsPlugin {
    roots: FileRoots,
}

impl FsPlugin {
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
    ) -> FsPlugin {
        FsPlugin {
            roots: FileRoots::new(roots, protected),
        }
    }
}

#[async_trait]
impl Plugin for FsPlugin {
    fn name(&self) -> &'static str {
        "fs"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let fs: Arc<dyn FsProvider> = Arc::new(LocalFs);
        ctx.keep(ctx.provide::<Fs>(fs.clone())?);

        let ledger = Arc::new(ReadLedger::default());
        let tools = ctx.require::<Tools>()?;
        // Built from `ctx`, so the spill store is looked up at call time, not construction time.
        let overflow = Overflow::new(ctx);

        ctx.keep(tools.register(Arc::new(Read::new(
            fs.clone(),
            self.roots.clone(),
            ledger.clone(),
            overflow.clone(),
        ))));
        ctx.keep(tools.register(Arc::new(Write::new(fs.clone(), self.roots.clone()))));
        ctx.keep(tools.register(Arc::new(Edit::new(fs, self.roots.clone()))));
        ctx.keep(tools.register(Arc::new(GlobTool::new(self.roots.clone()))));
        ctx.keep(tools.register(Arc::new(Grep::new(self.roots.clone(), overflow.clone()))));
        ctx.keep(tools.register(Arc::new(ListDir::new(self.roots.clone(), overflow))));

        ctx.keep(ctx.on_waterfall(Arc::new(ReadBeforeEdit::new(ledger, self.roots.clone()))));
        Ok(())
    }
}
