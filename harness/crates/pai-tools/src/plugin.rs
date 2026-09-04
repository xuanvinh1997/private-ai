//! Mount the registry into the tree.
//! A seam rather than a global, because each session may hold a different tool set and tests
//! need their own tree. The spill store comes along, or one wide `grep` fills the window.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::builtin::spill_read::SpillRead;
use crate::builtin::todo::TodoWrite;
use crate::registry::ToolRegistry;
use crate::seam::{Spill, Tools};
use crate::spill::{MemorySpillStore, SpillStore};

#[derive(Default)]
pub struct ToolsPlugin;

#[async_trait]
impl Plugin for ToolsPlugin {
    fn name(&self) -> &'static str {
        "tools"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let registry = ToolRegistry::new(ctx);
        // `todo_write` lives here, not in its own plugin: it touches no disk or network and has nothing to switch off.
        ctx.keep(registry.register(Arc::new(TodoWrite::new())));
        // `spill_read` registers with the store because it is the other half: stored text nobody can fetch is a lie.
        ctx.keep(registry.register(Arc::new(SpillRead::new(ctx))));
        ctx.keep(ctx.provide::<Tools>(registry)?);
        let spill: Arc<dyn SpillStore> = Arc::new(MemorySpillStore::default());
        ctx.keep(ctx.provide::<Spill>(spill)?);
        Ok(())
    }
}
