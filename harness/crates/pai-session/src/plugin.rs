//! Cắm sổ tay phiên vào cây plugin.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::store::{NoTitle, SessionStore, SessionTitle, Sessions};

/// Cắm một provider cho seam [`Sessions`], và provider duy nhất của v0.1 cho
/// [`SessionTitle`].
///
/// Kho được truyền vào chứ không dựng ở đây: chọn SQLite hay một kho khác là quyết định
/// của nơi ráp ứng dụng, không phải của plugin này.
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
