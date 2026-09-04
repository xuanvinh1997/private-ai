//! Command output is large output too. A build or a red test suite emits hundreds of KiB and
//! the tail holds the outcome, so head-only truncation would hide every command's result.

use std::path::PathBuf;
use std::sync::Arc;

use pai_core::Context;
use pai_sandbox::Policy;
use pai_shell::jobs::Jobs;
use pai_shell::provider::{LocalShell, ShellExecutor};
use pai_shell::tools::bash::Bash;
use pai_tools::{
    Invocation, MemorySpillStore, Overflow, Spill, SpillRef, SpillStore, Tool, ToolName,
};
use serde_json::{Map, Value, json};

fn call(args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from("bash"), "c1", map)
}

/// Long output folds rather than truncates: head and tail survive and the full text is stored.
#[tokio::test]
async fn very_long_bash_output_is_folded_and_spilled_to_the_store() {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("the spill store mounts"),
    );

    let shell: Arc<dyn ShellExecutor> = Arc::new(LocalShell::new(
        ctx.clone(),
        Policy::danger_full_access("/tmp"),
    ));
    let bash = Bash::new(
        shell,
        Arc::new(Jobs::default()),
        PathBuf::from("/tmp"),
        Overflow::new(&ctx).with_budget(200),
    );

    let outcome = bash
        .execute(&call(json!({
            "command": "seq 1 5000 | sed 's/^/dong /'; echo XONG-CUOI"
        })))
        .await
        .expect("runs");

    assert!(!outcome.is_error, "{}", outcome.content);
    assert!(
        outcome.content.contains("dong 1\n"),
        "lost the head:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("XONG-CUOI"),
        "lost the tail — this is where the command's outcome lives:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "truncated silently:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("`spill_read` với `id:"),
        "does not say how to fetch the full text:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("| tail -n 200"),
        "does not say how to filter inside the command itself:\n{}",
        outcome.content
    );

    let handle: SpillRef = serde_json::from_value(
        outcome
            .meta
            .get("spill")
            .cloned()
            .expect("a folded result must carry a ticket"),
    )
    .expect("the ticket parses");
    let full = store.read(&handle).expect("the ticket is still valid");
    assert!(full.contains("dong 2500"), "the middle must not be lost");
    assert!(
        outcome.content.len() < full.len() / 4,
        "what went to the model is still long: {} bytes",
        outcome.content.len()
    );
}

/// Output within budget passes through untouched, so "always fold" cannot pass the test above.
#[tokio::test]
async fn short_output_passes_through_and_mints_no_ticket() {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("the spill store mounts"),
    );

    let shell: Arc<dyn ShellExecutor> = Arc::new(LocalShell::new(
        ctx.clone(),
        Policy::danger_full_access("/tmp"),
    ));
    let bash = Bash::new(
        shell,
        Arc::new(Jobs::default()),
        PathBuf::from("/tmp"),
        Overflow::new(&ctx).with_budget(200),
    );

    let outcome = bash
        .execute(&call(json!({ "command": "echo xin-chao" })))
        .await
        .expect("runs");

    assert!(outcome.content.contains("xin-chao"));
    assert!(
        !outcome.content.contains("đã cắt bớt"),
        "{}",
        outcome.content
    );
    assert!(outcome.meta.get("spill").is_none());
    assert!(store.is_empty(), "nothing truncated means nothing stored");
}
