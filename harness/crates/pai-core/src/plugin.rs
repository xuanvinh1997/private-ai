//! Plugins: a plugin holds no privilege, it takes a `Context` and contributes services,
//! listeners, or both. Registrations belong to its effect scope, so unloading just disposes.

use async_trait::async_trait;

use crate::context::Context;

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// The name used in logs and `--dump-config`.
    fn name(&self) -> &'static str;

    /// Mount into the tree. `ctx` is already this plugin's own context.
    async fn apply(&self, ctx: &Context) -> anyhow::Result<()>;
}
