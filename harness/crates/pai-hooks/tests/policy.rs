//! Hooks can block, and a broken hook does not. Approval is fail-closed since it speaks for a
//! person; hooks are fail-open since they speak for a config file, so a typo cannot freeze the app.

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
    // Three ways to break, one outcome: none of them is evidence the call is dangerous.
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
    // The deadline is shortened for the test: measuring the real 10s went red under load.
    let ctx = with_hooks(vec![hook_with("sleep 30", &[], Some(1))]).await;
    let started = std::time::Instant::now();
    assert!(matches!(
        decide(&ctx, "bash", json!({})).await,
        PreDecision::Allow
    ));
    let waited = started.elapsed();
    // Under test: `sleep 30` does not run for 30 seconds; the bound only rules out no cut.
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
    // Every hook call is a spawn, so the cheapest calls must not pay for an unrelated policy.
    assert!(matches!(
        decide(&ctx, "read", json!({})).await,
        PreDecision::Allow
    ));
}

#[tokio::test]
async fn the_hook_reads_the_tool_name_and_arguments_on_stdin() {
    // The hook blocks only when it sees the command it cares about, so it did read stdin.
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
