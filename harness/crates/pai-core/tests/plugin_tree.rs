//! Exercise the core in exactly the shape the harness will use it: a seam, a plugin that
//! contributes to that seam, and a middleware that cuts in to veto.
//!
//! If those three do not fit together, everything built on top is wrong, so these tests run
//! before there are any real tools.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Notify, ServiceKey, Waterfall};

// --- a seam -------------------------------------------------------------------------

#[async_trait]
trait ToolRegistry: Send + Sync {
    async fn call(&self, name: &str, args: &str) -> String;
}

enum Tools {}
impl ServiceKey for Tools {
    type Api = dyn ToolRegistry;
    const NAME: &'static str = "tools";
}

struct EchoTools;

#[async_trait]
impl ToolRegistry for EchoTools {
    async fn call(&self, name: &str, args: &str) -> String {
        format!("{name}({args})")
    }
}

// --- a waterfall --------------------------------------------------------------------

struct ToolCall {
    name: String,
    args: String,
}

enum PreExecute {}
impl Waterfall for PreExecute {
    const NAME: &'static str = "tools/pre-execute";
    type Req = ToolCall;
    type Out = Result<String, String>;
}

/// Block one tool and do not delegate — a veto in the literal sense.
struct DenyGate {
    deny: &'static str,
}

impl Middleware<PreExecute> for DenyGate {
    fn call<'a>(
        &'a self,
        req: &'a mut ToolCall,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, Result<String, String>> {
        async move {
            if req.name == self.deny {
                // A refusal is text, not an error: the model has to be able to read why.
                return Err(format!(
                    "tool `{}` không được phép trong phạm vi này",
                    req.name
                ));
            }
            next.run(req).await
        }
        .boxed()
    }
}

/// Edit the request and still delegate — the cooperative branch, quite unlike the veto.
struct Rewrite;

impl Middleware<PreExecute> for Rewrite {
    fn call<'a>(
        &'a self,
        req: &'a mut ToolCall,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, Result<String, String>> {
        async move {
            req.args = format!("{}+đã-sửa", req.args);
            next.run(req).await
        }
        .boxed()
    }
}

// --- an observation event -----------------------------------------------------------

enum ToolCalled {}
impl Notify for ToolCalled {
    const NAME: &'static str = "tool/call";
    type Payload = String;
}

// --- putting it together ------------------------------------------------------------

async fn dispatch(ctx: &Context, name: &str, args: &str) -> Result<String, String> {
    let mut req = ToolCall {
        name: name.into(),
        args: args.into(),
    };
    ctx.waterfall::<PreExecute, _>(&mut req, |req| {
        let ctx = ctx.clone();
        async move {
            let tools = ctx.require::<Tools>().map_err(|e| e.to_string())?;
            Ok(tools.call(&req.name, &req.args).await)
        }
        .boxed()
    })
    .await
}

#[tokio::test]
async fn seams_middleware_and_effects_fit_together() {
    let root = Context::root();

    // A plugin provides the seam's provider.
    let tools_plugin = root.plugin("tools");
    let provided = tools_plugin
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("mounts");
    tools_plugin.keep(provided);

    // No middleware: straight through to the body.
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs)".into())
    );

    // A policy plugin cuts in. It knows nothing about `EchoTools`.
    let gate = root.plugin("gate");
    let gate_guard = gate.on_waterfall::<PreExecute>(Arc::new(DenyGate { deny: "bash" }));
    gate.keep(gate_guard);

    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs)".into())
    );
    assert!(dispatch(&root, "bash", "rm -rf /").await.is_err());

    // Another plugin edits the request and still delegates.
    let rewrite = root.plugin("rewrite");
    let rewrite_guard = rewrite.on_waterfall::<PreExecute>(Arc::new(Rewrite));
    rewrite.keep(rewrite_guard);
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs+đã-sửa)".into())
    );

    // Dispose the policy plugin: its registration disappears, the rest is untouched.
    gate.effects().dispose().await;
    assert!(dispatch(&root, "bash", "ls").await.is_ok());
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs+đã-sửa)".into())
    );
}

#[tokio::test]
async fn disposing_a_plugin_withdraws_its_registrations() {
    let root = Context::root();
    let plugin = root.plugin("tools");
    let guard = plugin
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("mounts");
    plugin.keep(guard);

    assert!(root.get::<Tools>().is_some());
    plugin.effects().dispose().await;
    assert!(root.get::<Tools>().is_none());
}

#[tokio::test]
async fn two_providers_for_one_seam_in_one_realm_is_a_config_error() {
    let root = Context::root();
    let first = root
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("mounts");
    assert!(root.provide::<Tools>(Arc::new(EchoTools)).is_err());

    // But in a different realm they coexist.
    let isolated = root.isolate::<Tools>();
    let second = isolated
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("its own realm");

    first.leak();
    second.leak();
}

#[tokio::test]
async fn waiting_for_a_service_replaces_hand_ordering_startup() {
    let root = Context::root();
    let waiter = root.clone();
    let task = tokio::spawn(async move { waiter.wait_for::<Tools>().await.call("x", "").await });

    // Mounted late. The waiter has to wake itself.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    root.provide::<Tools>(Arc::new(EchoTools))
        .expect("mounts")
        .leak();

    assert_eq!(task.await.unwrap(), "x()");
}

#[tokio::test]
async fn a_scoped_listener_does_not_reach_other_agents() {
    let root = Context::root();
    let seen = Arc::new(AtomicUsize::new(0));

    let agent = root.scoped("agent-a");
    let counter = seen.clone();
    let guard = agent.on_notify::<ToolCalled>(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    agent.keep(guard);

    // Emit at the root: the child agent's listener does not receive it, because events
    // flow up, not down.
    root.notify::<ToolCalled>(&"read".to_string());
    assert_eq!(seen.load(Ordering::SeqCst), 0);

    // Emit inside its own scope: received.
    agent.notify::<ToolCalled>(&"read".to_string());
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}
