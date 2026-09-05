//! Plugs the web layer into the tree: one plugin, two tools, one provider seam.
//!
//! An application-layer plugin, not a project-layer one. `pai-rag`'s plugin is rebuilt on every
//! project switch because it captures that project's folder; the web has no such thing to capture.
//! Mounting it once also keeps one warm connection pool for the whole session, which a
//! per-project mount would throw away every time the user changed windows.
//!
//! Neither tool is registered conditionally on having an API key: `web.fetch` needs none, and
//! `web.search` without one has to say so out loud when it is called. A tool that silently
//! disappears from the list is a tool the user cannot ask why is missing.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::fetch::{Fetcher, Limits};
use crate::guard::Guard;
use crate::search::{Brave, SearchProvider};
use crate::tools::{WebFetch, WebSearch};

pub struct WebPlugin {
    /// The Brave key, when the application has one. `None` and `web.search` still registers; it
    /// just fails with an explanation naming the environment variable.
    brave_key: Option<String>,
    /// Overrides the default provider. The seam exists for a future provider swap; the tests use
    /// it too, which is how they run with no key and no network.
    provider: Option<Arc<dyn SearchProvider>>,
    limits: Limits,
}

impl Default for WebPlugin {
    fn default() -> WebPlugin {
        WebPlugin::new()
    }
}

impl WebPlugin {
    /// Reads the key from the environment. Doing it here rather than inside [`Brave`] keeps the
    /// provider testable and keeps configuration in one layer.
    pub fn new() -> WebPlugin {
        WebPlugin::with_brave_key(std::env::var(crate::search::brave::KEY_ENV).ok())
    }

    pub fn with_brave_key(brave_key: Option<String>) -> WebPlugin {
        WebPlugin {
            brave_key,
            provider: None,
            limits: Limits::default(),
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn SearchProvider>) -> WebPlugin {
        self.provider = Some(provider);
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> WebPlugin {
        self.limits = limits;
        self
    }
}

#[async_trait]
impl Plugin for WebPlugin {
    fn name(&self) -> &'static str {
        "web"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let tools = ctx.require::<Tools>()?;

        let fetcher = Arc::new(Fetcher::new(Guard::strict(), self.limits)?);
        ctx.keep(tools.register(Arc::new(WebFetch::new(fetcher))));

        let provider = match &self.provider {
            Some(provider) => provider.clone(),
            // A separate client from the fetcher's: that one refuses to follow redirects, which is
            // right for a model-chosen URL and wrong for a fixed, trusted API endpoint. Timeouts
            // are not optional even so -- a hung TLS handshake would otherwise hold the tool until
            // the pipeline's own deadline noticed.
            None => Arc::new(Brave::new(
                reqwest::Client::builder()
                    .timeout(crate::search::brave::DEFAULT_TIMEOUT)
                    .connect_timeout(crate::search::brave::DEFAULT_TIMEOUT)
                    .build()?,
                self.brave_key.clone(),
            )),
        };
        if self.brave_key.is_none() && self.provider.is_none() {
            tracing::info!(
                "web.search chưa có khoá API ({}); tool vẫn đăng ký và sẽ báo lỗi khi được gọi",
                crate::search::brave::KEY_ENV
            );
        }
        ctx.keep(tools.register(Arc::new(WebSearch::new(provider))));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pai_tools::{ToolsPlugin, Tools};

    async fn mount(plugin: WebPlugin) -> Context {
        let ctx = Context::root();
        ToolsPlugin
            .apply(&ctx.plugin("tools"))
            .await
            .expect("mount tools");
        plugin
            .apply(&ctx.plugin("web"))
            .await
            .expect("mount web");
        ctx
    }

    #[tokio::test]
    async fn cam_ca_hai_tool_vao_cay() {
        // Explicitly keyless, so the test does not depend on the developer's environment.
        let ctx = mount(WebPlugin::with_brave_key(None)).await;
        let names: Vec<String> = ctx
            .require::<Tools>()
            .expect("registry")
            .schemas(None)
            .into_iter()
            .map(|schema| schema.name.as_str().to_string())
            .collect();
        assert!(names.contains(&"web.fetch".to_string()), "{names:?}");
        // Registered even with no key: a tool that vanishes is a tool nobody can ask about.
        assert!(names.contains(&"web.search".to_string()), "{names:?}");
    }

    #[tokio::test]
    async fn go_plugin_thi_tool_bien_mat() {
        let ctx = Context::root();
        ToolsPlugin
            .apply(&ctx.plugin("tools"))
            .await
            .expect("mount tools");
        let web = ctx.plugin("web");
        WebPlugin::with_brave_key(None)
            .apply(&web)
            .await
            .expect("mount web");
        let tools = ctx.require::<Tools>().expect("registry");
        assert_eq!(tools.schemas(None).iter().filter(|s| s.name.as_str().starts_with("web.")).count(), 2);

        // Unloading is disposing the plugin's effect scope; nothing else has to remember to unregister.
        web.effects().dispose().await;
        assert_eq!(tools.schemas(None).iter().filter(|s| s.name.as_str().starts_with("web.")).count(), 0);
    }
}
