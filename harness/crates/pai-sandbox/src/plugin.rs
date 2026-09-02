//! Mount the sandbox into the tree.
//!
//! The plugin does not choose the policy — it only mounts a provider. The policy
//! (`read-only` or `workspace-write`) belongs to the session, because one machine can run a
//! read-only agent next to an agent allowed to edit the repo, and both share exactly one
//! sandbox.
//!
//! Having no provider at all is a **valid** state, not a config error: test runs and runs on
//! unsupported operating systems both land there. Consumers have to handle an empty seam,
//! and `for_this_machine` always returns a provider with a reason rather than returning
//! nothing — "nobody answered" and "the answer is that confinement is unavailable" are two
//! different sentences as far as the approval dialog is concerned.

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

    /// A caller-supplied provider. For tests, and for remote runs where the sandbox lives
    /// at the far end rather than on this machine.
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
            // Logged at `warn`, not `info`: an installation running unconfined with
            // nobody noticing is precisely the situation `Enforcement` exists to prevent.
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
