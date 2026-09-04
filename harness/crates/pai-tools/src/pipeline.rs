//! The guarded execution pipeline: pre-execute, approval, guards, execute, post-execute,
//! finalize, result. Guards run after approval so a click cannot open what policy closed,
//! refusals still pass through post-execute, and the outer `execute` never returns `Result`.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Notify, ScopeKey, Waterfall};
use serde_json::{Map, Value, json};

use crate::budget::{DEFAULT_TOKEN_BUDGET, Overflow};
use crate::name::ToolName;
use crate::registry::{Resolution, ToolRegistry};
use crate::schema::ToolMeta;
use crate::seam::{Approval, Elicitation};
use crate::tool::{Invocation, Tool, ToolOutcome};

/// How long an unanswered approval counts as "no"; without a deadline a hidden dialog holds the turn forever.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

// --- decisions ---------------------------------------------------------------------------

/// The `tools/pre-execute` result.
#[derive(Clone, Debug, PartialEq)]
pub enum PreDecision {
    Allow,
    /// The reason goes straight to the model, so it has to say what to do instead.
    Deny(String),
    /// Hand the decision to the user.
    Ask {
        reason: String,
    },
}

/// The `tools/post-execute` result; replacing an outcome happens through `req.outcome`, not a second path here.
#[derive(Clone, Debug, PartialEq)]
pub enum PostDecision {
    Accept { additional_context: Vec<String> },
    Block { reason: String },
}

// --- events ------------------------------------------------------------------------------

pub struct PreRequest {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    pub meta: ToolMeta,
}

pub struct PostRequest {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    pub outcome: ToolOutcome,
}

/// A frozen result.
pub struct ResolvedCall {
    pub name: ToolName,
    pub call_id: String,
    pub outcome: ToolOutcome,
    pub additional_context: Vec<String>,
}

pub enum PreExecute {}
impl Waterfall for PreExecute {
    const NAME: &'static str = "tools/pre-execute";
    type Req = PreRequest;
    type Out = PreDecision;
}

pub enum Execute {}
impl Waterfall for Execute {
    const NAME: &'static str = "tools/execute";
    type Req = Invocation;
    type Out = ToolOutcome;
}

pub enum PostExecute {}
impl Waterfall for PostExecute {
    const NAME: &'static str = "tools/post-execute";
    type Req = PostRequest;
    type Out = PostDecision;
}

pub enum ToolResult {}
impl Notify for ToolResult {
    const NAME: &'static str = "tools/result";
    type Payload = ResolvedCall;
}

// --- guards ------------------------------------------------------------------------------

/// A monotonic guard with no allow branch, so registration order cannot turn a refusal into a pass.
#[async_trait]
pub trait ToolGuard: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    /// `Some(reason)` refuses; `None` has no opinion.
    async fn check(&self, call: &Invocation, meta: &ToolMeta) -> Option<String>;
}

// --- approval ----------------------------------------------------------------------------

pub struct ApprovalRequest {
    pub name: ToolName,
    pub call_id: String,
    pub reason: String,
    pub arguments: Map<String, Value>,
    pub meta: ToolMeta,
}

/// Ask the user, once per call; only a `bool`, because a remembered answer answers a question the user never heard.
#[async_trait]
pub trait Approver: Send + Sync + 'static {
    async fn approve(&self, request: &ApprovalRequest) -> bool;
}

// --- the pipeline ------------------------------------------------------------------------

/// What the model reads when it calls a tool it may not; one sentence for both "denied" and "unknown", so it cannot probe.
pub fn not_available(name: &ToolName) -> String {
    format!("Tool `{name}` không khả dụng với agent này.")
}

/// Coerce a closure into the higher-ranked bound the waterfall tail needs; inference otherwise picks a concrete lifetime.
fn tail<E: Waterfall, F>(f: F) -> F
where
    F: for<'r> Fn(&'r mut E::Req) -> BoxFuture<'r, E::Out> + Send + Sync,
{
    f
}

pub struct ToolPipeline {
    ctx: Context,
    registry: Arc<ToolRegistry>,
    scope: Option<ScopeKey>,
    /// The budget in approximate tokens, not lines or characters. See [`crate::budget`].
    budget: usize,
    approval_timeout: Duration,
}

impl ToolPipeline {
    /// The scope comes from `ctx`, so a pipeline always carries the restrictions of the agent that built it.
    pub fn new(ctx: &Context, registry: Arc<ToolRegistry>) -> ToolPipeline {
        ToolPipeline {
            scope: ctx.scope_key(),
            ctx: ctx.clone(),
            registry,
            budget: DEFAULT_TOKEN_BUDGET,
            approval_timeout: APPROVAL_TIMEOUT,
        }
    }

    /// The budget in approximate tokens; the final ceiling, applied even to tools that apply none themselves.
    pub fn with_token_budget(mut self, tokens: usize) -> ToolPipeline {
        self.budget = tokens.max(1);
        self
    }

    pub fn with_approval_timeout(mut self, timeout: Duration) -> ToolPipeline {
        self.approval_timeout = timeout;
        self
    }

    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// The outer edge; returns no `Result` — see the module header.
    pub async fn execute(&self, call_id: &str, raw_name: &str, arguments: Value) -> ToolOutcome {
        // The second filter, on the decoded name and before anything tool-owned: a guessed wire name stops here.
        let (tool, name) = match self.registry.resolve(self.scope, raw_name) {
            Resolution::Found(tool, name) => (tool, name),
            Resolution::Denied(name) => {
                return ToolOutcome::error(not_available(&name))
                    .with_meta("refusal", json!("denied"))
                    .with_meta("tool", json!(name.as_str()));
            }
            Resolution::Unknown(name) => {
                return ToolOutcome::error(not_available(&name))
                    .with_meta("refusal", json!("unknown"))
                    .with_meta("tool", json!(name.as_str()));
            }
        };

        let meta = tool.meta();
        let mut args = match arguments {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            _ => {
                return ToolOutcome::error(format!("Tool `{name}` cần tham số dạng object JSON."));
            }
        };
        self.registry.apply_pins(self.scope, &mut args);

        let denial = self.gate(&name, call_id, &meta, &mut args).await;

        let mut inv = Invocation::new(name.clone(), call_id, args)
            .with_elicitor(self.ctx.get::<Elicitation>());

        let outcome = match denial {
            Some(text) => ToolOutcome::error(text).with_meta("refusal", json!("policy")),
            None => self.run(&tool, &meta, &mut inv).await,
        };

        self.settle(tool.as_ref(), name, call_id, inv.arguments, outcome)
            .await
    }

    /// pre-execute, then approval, then guards; `Some(reason)` means the tool body never runs.
    async fn gate(
        &self,
        name: &ToolName,
        call_id: &str,
        meta: &ToolMeta,
        args: &mut Map<String, Value>,
    ) -> Option<String> {
        let mut pre = PreRequest {
            name: name.clone(),
            call_id: call_id.to_string(),
            arguments: std::mem::take(args),
            meta: meta.clone(),
        };
        let decision = self
            .ctx
            .waterfall::<PreExecute, _>(&mut pre, |_| async { PreDecision::Allow }.boxed())
            .await;

        *args = std::mem::take(&mut pre.arguments);
        // Re-pin after the waterfall: middleware may edit arguments but must not unpin, or a harmless hook becomes a bypass.
        self.registry.apply_pins(self.scope, args);

        match decision {
            PreDecision::Deny(reason) => return Some(reason),
            PreDecision::Ask { reason } => {
                let request = ApprovalRequest {
                    name: name.clone(),
                    call_id: call_id.to_string(),
                    reason: reason.clone(),
                    arguments: args.clone(),
                    meta: meta.clone(),
                };
                if !self.ask(&request).await {
                    return Some(format!("Người dùng không cho phép `{name}`: {reason}"));
                }
            }
            PreDecision::Allow => {}
        }

        // Guards run after approval: a user's "allow" cannot open what policy closed.
        let probe = Invocation::new(name.clone(), call_id, args.clone());
        for guard in self.registry.guards(self.scope) {
            let checking = AssertUnwindSafe(guard.check(&probe, meta)).catch_unwind();
            match checking.await {
                Ok(None) => {}
                Ok(Some(reason)) => {
                    // Stop at the first refusal; the rest cannot change the answer, since no guard can allow.
                    tracing::info!(tool = %name, guard = guard.name(), "guard refused");
                    return Some(reason);
                }
                Err(_) => {
                    // A panicking guard reached no conclusion, and no conclusion has to mean refuse.
                    tracing::error!(tool = %name, guard = guard.name(), "guard panicked");
                    return Some(format!(
                        "Canh gác `{}` không kiểm tra được `{name}`, nên lệnh gọi bị từ chối.",
                        guard.name()
                    ));
                }
            }
        }
        None
    }

    /// Ask for approval; fail-closed on all three branches: nobody to ask, timed out, or the approver panicked.
    async fn ask(&self, request: &ApprovalRequest) -> bool {
        let Some(approver) = self.ctx.get::<Approval>() else {
            tracing::warn!(tool = %request.name, "no approver is mounted: refusing");
            return false;
        };
        let asking = AssertUnwindSafe(approver.approve(request)).catch_unwind();
        match tokio::time::timeout(self.approval_timeout, asking).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => {
                tracing::error!(tool = %request.name, "the approver panicked: refusing");
                false
            }
            Err(_) => {
                tracing::warn!(tool = %request.name, "approval timed out: refusing");
                false
            }
        }
    }

    /// `tools/execute`, wrapped in a timeout.
    async fn run(
        &self,
        tool: &Arc<dyn Tool>,
        meta: &ToolMeta,
        inv: &mut Invocation,
    ) -> ToolOutcome {
        let cancel = inv.cancel_token();
        let body = tail::<Execute, _>(move |call: &mut Invocation| {
            let tool = tool.clone();
            async move {
                match tool.execute(&*call).await {
                    Ok(outcome) => outcome,
                    // A tool body may return `Err`; by this point it becomes text.
                    Err(err) => ToolOutcome::error(format!("Tool `{}` lỗi: {err}", call.name)),
                }
            }
            .boxed()
        });

        let finished = {
            let running = self.ctx.waterfall::<Execute, _>(inv, body);
            tokio::time::timeout(meta.timeout, AssertUnwindSafe(running).catch_unwind()).await
        };

        match finished {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                ToolOutcome::error(format!("Tool `{}` hoảng loạn và bị dừng lại.", inv.name))
                    .with_meta("failure", json!("panic"))
            }
            Err(_) => {
                // Cancel so the body abandons its work rather than running on for a result nobody will take.
                cancel.cancel();
                ToolOutcome::error(format!(
                    "Tool `{}` quá {} giây và bị dừng lại.",
                    inv.name,
                    meta.timeout.as_secs()
                ))
                .with_meta("failure", json!("timeout"))
            }
        }
    }

    /// post-execute, then finalize, then spill, then notify.
    async fn settle(
        &self,
        tool: &dyn Tool,
        name: ToolName,
        call_id: &str,
        arguments: Map<String, Value>,
        outcome: ToolOutcome,
    ) -> ToolOutcome {
        let mut post = PostRequest {
            name: name.clone(),
            call_id: call_id.to_string(),
            arguments,
            outcome,
        };
        let decision = self
            .ctx
            .waterfall::<PostExecute, _>(&mut post, |_| {
                async {
                    PostDecision::Accept {
                        additional_context: Vec::new(),
                    }
                }
                .boxed()
            })
            .await;

        let mut outcome = post.outcome;
        let additional = match decision {
            PostDecision::Accept { additional_context } => additional_context,
            PostDecision::Block { reason } => {
                outcome = ToolOutcome::error(reason).with_meta("refusal", json!("post"));
                Vec::new()
            }
        };

        // Synchronous and content-only, so a tool cannot flip `is_error` after policy has run.
        let mut content = std::mem::take(&mut outcome.content);
        if std::panic::catch_unwind(AssertUnwindSafe(|| tool.finalize(&mut content))).is_err() {
            tracing::error!(tool = %name, "finalize panicked; keeping the content unchanged");
        }
        outcome.content = content;

        self.spill(&name, &mut outcome);

        if !additional.is_empty() {
            outcome
                .meta
                .insert("additional_context".into(), json!(additional.clone()));
        }

        // Frozen: from here the outcome is read-only.
        self.ctx.notify::<ToolResult>(&ResolvedCall {
            name,
            call_id: call_id.to_string(),
            outcome: outcome.clone(),
            additional_context: additional,
        });
        outcome
    }

    /// The final ceiling, a safety net for tools that budget nothing themselves, such as third-party MCP tools.
    fn spill(&self, name: &ToolName, outcome: &mut ToolOutcome) {
        let overflow = Overflow::new(&self.ctx).with_budget(self.budget);
        let full = std::mem::take(&mut outcome.content);
        // The hint is deliberately vague: the pipeline does not know how this tool paginates.
        let folded = overflow.fold(name, full, |_| {
            "Gọi lại tool với tham số hẹp hơn nếu bạn chỉ cần một phần.".to_string()
        });
        outcome.content = folded.content;
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
    }
}
