//! Plugins.
//!
//! A plugin holds no privilege: it receives a `Context` and contributes services,
//! listeners, or both. Every registration it makes belongs to its effect scope, so
//! unloading is just calling disposers — there is no registry to clean up by hand.

use async_trait::async_trait;

use crate::context::Context;

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// The name used in logs and `--dump-config`.
    fn name(&self) -> &'static str;

    /// Mount into the tree. `ctx` is already this plugin's own context.
    async fn apply(&self, ctx: &Context) -> anyhow::Result<()>;
}
