//! Mount the sandbox into the tree. The plugin mounts a provider only; the policy belongs to
//! the session, so one machine can run a read-only agent beside an editing one. An empty seam
//! is valid, and `for_this_machine` always answers with a provider carrying a reason.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::seam::{Sandbox, SandboxProvider, for_this_machine};

pub struct SandboxPlugin {
    provider: Arc<dyn SandboxProvider>,
}

impl Default for SandboxPlugin {
    fn default() -> SandboxPlugin {
        SandboxPlugin {
            provider: for_this_machine(),
        }
    }
}

impl SandboxPlugin {
    /// The provider for the running machine.
    pub fn new() -> SandboxPlugin {
        SandboxPlugin::default()
    }

    /// A caller-supplied provider, for tests and for runs sandboxed on a remote machine.
    pub fn with_provider(provider: Arc<dyn SandboxProvider>) -> SandboxPlugin {
        SandboxPlugin { provider }
    }
}

#[async_trait]
impl Plugin for SandboxPlugin {
    fn name(&self) -> &'static str {
        "sandbox"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let enforcement = self.provider.enforcement();
        match enforcement.reason() {
            // `warn`, not `info`: running unconfined unnoticed is what `Enforcement` prevents.
            Some(reason) => {
                tracing::warn!(
                    mode = enforcement.label(),
                    "process confinement is limited: {reason}"
                )
            }
            None => tracing::info!(mode = enforcement.label(), "process confinement is full"),
        }
        ctx.keep(ctx.provide::<Sandbox>(self.provider.clone())?);
        Ok(())
    }
}
