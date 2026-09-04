//! Wires the session log into the plugin tree.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::store::{NoTitle, SessionStore, SessionTitle, Sessions};

/// Provides the [`Sessions`] seam plus v0.1's only [`SessionTitle`]; the store is injected, since picking one is the app's call.
pub struct SessionPlugin {
    store: Arc<dyn SessionStore>,
}

impl SessionPlugin {
    pub fn new(store: Arc<dyn SessionStore>) -> SessionPlugin {
        SessionPlugin { store }
    }
}

#[async_trait]
impl Plugin for SessionPlugin {
    fn name(&self) -> &'static str {
        "pai-session"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        ctx.keep(ctx.provide::<Sessions>(self.store.clone())?);
        ctx.keep(ctx.provide::<SessionTitle>(Arc::new(NoTitle))?);
        Ok(())
    }
}
