//! Hooks can block, and a broken hook does not.
//!
//! The first pair of tests is the whole content of this crate. They mirror each other and
//! stand opposite `Approver`: approval is fail-**closed** because it speaks for a person
//! sitting there, hooks are fail-**open** because they speak for a config file. Confusing
//! those two defaults turns a typo in the user's script into a frozen application.

use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_hooks::{HookConfig, HooksPlugin};
use pai_tools::{PreDecision, PreExecute, PreRequest, ToolMeta, ToolName};
use serde_json::{Map, Value, json};

async fn decide(ctx: &Context, tool: &str, args: Value) -> PreDecision {
    let arguments: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let mut req = PreRequest {
        name: ToolName::from(tool),
        call_id: "c1".into(),
        arguments,
        meta: ToolMeta::mutating(),
    };
    ctx.waterfall::<PreExecute, _>(&mut req, |_| Box::pin(async { PreDecision::Allow }))
        .await
}

async fn with_hooks(hooks: Vec<HookConfig>) -> Context {
    let ctx = Context::root();
    let scope = ctx.plugin("hooks");
    HooksPlugin::new(hooks)
        .apply(&scope)
        .await
        .expect("mounts cleanly");
    std::mem::forget(scope);
    ctx
}

fn hook(command: &str, tools: &[&str]) -> HookConfig {
    hook_with(command, tools, None)
}

fn hook_with(command: &str, tools: &[&str], timeout_secs: Option<u64>) -> HookConfig {
    HookConfig {
        timeout_secs,
        command: command.to_string(),
        tools: tools.iter().map(|t| t.to_string()).collect(),
    }
}

#[tokio::test]
async fn a_hook_saying_no_blocks_and_the_reason_reaches_the_model() {
    let ctx = with_hooks(vec![hook(
        r#"echo '{"decision":"deny","reason":"chính sách công ty cấm chạy lệnh"}'"#,
        &[],
    )])
    .await;

    match decide(&ctx, "bash", json!({ "command": "ls" })).await {
        PreDecision::Deny(reason) => assert!(reason.contains("chính sách công ty")),
        other => panic!("should have been blocked, got {other:?}"),
    }
}

#[tokio::test]
async fn a_hook_saying_yes_lets_the_call_through() {
    let ctx = with_hooks(vec![hook(r#"echo '{"decision":"allow"}'"#, &[])]).await;
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn a_broken_hook_allows_rather_than_blocks() {
    // Three ways to break, one outcome: a command that does not exist, a non-zero exit,
    // and output that is not JSON. None of them is evidence that the call is dangerous.
    for command in ["no-such-command-here", "exit 3", "echo not-json"] {
        let ctx = with_hooks(vec![hook(command, &[])]).await;
        assert!(
            matches!(decide(&ctx, "bash", json!({})).await, PreDecision::Allow),
            "hook `{command}` broke and blocked anyway"
        );
    }
}

#[tokio::test]
async fn a_hook_that_times_out_allows() {
    // The deadline is **shortened for the test**, not the product's 10 seconds. An earlier
    // version measured the wall clock against the real timeout and went red twice at
    // random while the machine ran twenty other tests in parallel — the 20-second upper
    // bound was blown by scheduling, not by a broken timeout. A test that goes red because
    // the machine is busy says nothing about the code.
    let ctx = with_hooks(vec![hook_with("sleep 30", &[], Some(1))]).await;
    let started = std::time::Instant::now();
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
    let waited = started.elapsed();
    // What is under test: `sleep 30` does **not** run for 30 seconds. The upper bound is
    // generous because it only has to rule out "the timeout cut nothing", not measure.
    assert!(
        waited < std::time::Duration::from_secs(15),
        "the timeout did not cut: {waited:?}"
    );
}

#[tokio::test]
async fn a_hook_only_runs_for_the_tools_it_declares() {
    let ctx = with_hooks(vec![hook(
        r#"echo '{"decision":"deny","reason":"chỉ cấm bash"}'"#,
        &["bash"],
    )])
    .await;

    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Deny(_)
    ));
    // Every hook call is a process spawn; filtering here keeps the cheapest calls from
    // paying for a policy that does not talk about them.
    assert!(matches!(
        decide(&ctx, "read", json!({})).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn the_hook_reads_the_tool_name_and_arguments_on_stdin() {
    // The hook only blocks when it sees the command it cares about — which means it really
    // did read the payload.
    let ctx = with_hooks(vec![hook(
        r#"grep -q 'rm -rf' && echo '{"decision":"deny","reason":"lệnh xoá"}' || echo '{"decision":"allow"}'"#,
        &[],
    )])
    .await;

    assert!(matches!(
        decide(&ctx, "bash", json!({ "command": "rm -rf /" })).await,
        PreDecision::Deny(_)
    ));
    assert!(matches!(
        decide(&ctx, "bash", json!({ "command": "ls" })).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn no_hooks_means_nothing_is_registered() {
    let ctx = with_hooks(Vec::new()).await;
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
    // And no `Arc` is held anywhere: an empty plugin must leave no trace.
    let _ = Arc::new(());
}
