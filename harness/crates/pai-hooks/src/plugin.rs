//! Mount hooks into the tool pipeline.

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use pai_tools::{PreDecision, PreExecute, PreRequest};
use serde::Deserialize;

use crate::runner::{HookDecision, HookInput, run};

/// One hook, as written in the config file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookConfig {
    /// The command, run through `/bin/sh -c`.
    pub command: String,
    /// Only run for these tools. Empty = every tool.
    ///
    /// Filtered here rather than inside the hook, because every hook call is a process
    /// spawn — a hook that only cares about `bash` but gets invoked on every `read` slows
    /// down precisely the cheapest calls.
    #[serde(default)]
    pub tools: Vec<String>,
    /// This hook's own timeout, in seconds. Absent means [`HOOK_TIMEOUT`].
    ///
    /// Here for two separate reasons, both real. First, a hook that calls out to the
    /// network needs longer than one running `grep`, and a single number for both is wrong
    /// for at least one of them. Second, **the test suite needs to shorten it**: a timeout
    /// test that measures the wall clock goes red at random when the machine is running
    /// twenty other tests in parallel — which has happened twice in this repo.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl HookConfig {
    fn applies_to(&self, tool: &str) -> bool {
        self.tools.is_empty() || self.tools.iter().any(|name| name == tool)
    }
}

struct PreHooks {
    hooks: Vec<HookConfig>,
}

impl Middleware<PreExecute> for PreHooks {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        async move {
            let tool = req.name.as_str().to_string();
            for hook in self.hooks.iter().filter(|hook| hook.applies_to(&tool)) {
                let input = HookInput {
                    event: "pre-execute",
                    tool: &tool,
                    call_id: &req.call_id,
                    arguments: &req.arguments,
                    output: None,
                };
                // One hook saying no stops the loop; the remaining hooks are not asked.
                // The answer is already known, and going on only costs more processes.
                // This hook's own deadline, or the default. See `HookConfig::timeout_secs`.
                let deadline = hook
                    .timeout_secs
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(crate::runner::HOOK_TIMEOUT);
                if let Some(HookDecision::Deny { reason }) =
                    run(&hook.command, &input, deadline).await
                {
                    return PreDecision::Deny(reason);
                }
            }
            next.run(req).await
        }
        .boxed()
    }
}

pub struct HooksPlugin {
    hooks: Vec<HookConfig>,
}

impl HooksPlugin {
    pub fn new(hooks: Vec<HookConfig>) -> HooksPlugin {
        HooksPlugin { hooks }
    }
}

#[async_trait]
impl Plugin for HooksPlugin {
    fn name(&self) -> &'static str {
        "hooks"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        if self.hooks.is_empty() {
            return Ok(());
        }
        // Runs **before** every other layer, approval included: operator policy should
        // not have to wait on the user answering a question about something the policy has
        // already decided is not allowed.
        ctx.keep(ctx.on_waterfall_first(Arc::new(PreHooks {
            hooks: self.hooks.clone(),
        })));
        Ok(())
    }
}
