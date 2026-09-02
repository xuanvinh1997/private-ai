//! Budgets, spill, and the question "what is in this directory".
//!
//! Every test here locks a blind spot that was real: results truncated in silence, a cap
//! counting the wrong unit, a large repo making `grep` run forever, and an unfamiliar repo
//! where no tool could answer the model's first question.

use std::sync::Arc;

use pai_core::{Context, Plugin};
use pai_fs::path::FileRoots;
use pai_fs::provider::{FsProvider, LocalFs};
use pai_fs::tools::{grep::Grep, list::ListDir, read::Read};
use pai_fs::{FsPlugin, ReadLedger};
use pai_tools::Spill;
use pai_tools::{
    Invocation, MemorySpillStore, Overflow, Resolution, SpillRef, SpillStore, Tool, ToolName,
    ToolPipeline, ToolRegistry, Tools, ToolsPlugin,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

/// A tree with a spill store mounted — without one nothing is folded, by design.
fn tree() -> (Context, Arc<MemorySpillStore>) {
    let ctx = Context::root();
    let store = MemorySpillStore::new();
    ctx.keep(
        ctx.provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
            .expect("the spill store mounts"),
    );
    (ctx, store)
}

fn bench() -> (TempDir, FileRoots) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("root canonicalises");
    let roots = FileRoots::new([root.clone()], [root.join("bi-mat")]);
    (dir, roots)
}

fn spill_of(outcome: &pai_tools::ToolOutcome) -> SpillRef {
    serde_json::from_value(
        outcome
            .meta
            .get("spill")
            .cloned()
            .expect("a folded result must carry a ticket"),
    )
    .expect("the ticket parses")
}

// --- 1. reading a file that blows the budget --------------------------------------------

/// Locks in: **truncation is folding, not discarding.** The result must have both head and
/// tail, must carry a ticket, and must say *in words the model reads* how to get the rest.
///
/// The assertions go at the real strings rather than at "some field exists": a
/// `truncated = true` field whose content says nothing still leaves the model concluding it
/// saw everything.
#[tokio::test]
async fn reading_over_budget_keeps_head_and_tail_and_says_how_to_read_on() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let file = dir.path().canonicalize().unwrap().join("dai.txt");
    let content: String = (1..=4000).map(|n| format!("dòng số {n}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let read = Read::new(
        Arc::new(LocalFs) as Arc<dyn FsProvider>,
        roots,
        Arc::new(ReadLedger::default()),
        Overflow::new(&ctx).with_budget(200),
    );
    let outcome = read
        .execute(&call(
            "read",
            // An explicit `limit`: the budget is an **independent** ceiling, not another
            // way of writing `limit`. Asking for all 4000 lines and still getting folded is
            // what proves that.
            json!({ "file_path": file.display().to_string(), "limit": 4000 }),
        ))
        .await
        .expect("reads");

    // The head.
    assert!(
        outcome.content.contains("dòng số 1\n"),
        "lost the head:\n{}",
        outcome.content
    );
    // The tail. Without it the model does not know where the file ends.
    assert!(
        outcome.content.contains("dòng số 4000"),
        "lost the tail:\n{}",
        outcome.content
    );
    // Instructions for getting the rest, stated in words and **specifically**.
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "did not say it truncated:\n{}",
        outcome.content
    );
    assert!(
        outcome
            .content
            .contains("`read` với `file_path` như cũ và `offset:"),
        "did not say how to read on:\n{}",
        outcome.content
    );
    assert!(
        outcome.content.contains("`spill_read` với `id:"),
        "did not say how to fetch the full text:\n{}",
        outcome.content
    );

    // The full text survives intact.
    let handle = spill_of(&outcome);
    let full = store.read(&handle).expect("the ticket is still valid");
    assert!(
        full.contains("dòng số 2000"),
        "the middle has to stay in the store"
    );
    assert!(
        outcome.content.len() < full.len() / 2,
        "what went to the model is still long"
    );
}

// --- 2. counting lines counts the wrong thing -------------------------------------------

/// Locks in: **the budget measures bytes, not lines.** Five lines pass every line-based cap,
/// but these five lines weigh 15 KiB.
#[tokio::test]
async fn few_but_very_long_lines_are_still_folded_by_the_budget() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let file = dir.path().canonicalize().unwrap().join("mot-dong.json");
    let content: String = (0..5)
        .map(|n| format!("{}{}\n", (b'a' + n) as char, "x".repeat(3000)))
        .collect();
    std::fs::write(&file, &content).unwrap();

    let read = Read::new(
        Arc::new(LocalFs) as Arc<dyn FsProvider>,
        roots,
        Arc::new(ReadLedger::default()),
        Overflow::new(&ctx).with_budget(200),
    );
    let outcome = read
        .execute(&call(
            "read",
            json!({ "file_path": file.display().to_string() }),
        ))
        .await
        .expect("reads");

    // Only five lines — a "256 lines" cap or `limit: 2000` would let the whole file through.
    let read_meta = outcome.meta.get("read").expect("read meta is present");
    assert_eq!(read_meta["total_lines"], json!(5));
    assert!(
        outcome.content.contains("đã cắt bớt"),
        "few lines but long ones still have to be folded:\n{}",
        &outcome.content[..200.min(outcome.content.len())]
    );

    let handle = spill_of(&outcome);
    assert!(
        store.read(&handle).map(|s| s.len()).unwrap_or(0) > 15_000,
        "the full text has to survive in the store"
    );
    assert!(
        outcome.content.len() < 2_000,
        "what is sent has to sit near the 200-token budget, currently {} bytes",
        outcome.content.len()
    );
}

// --- 3. grep over a large repo ----------------------------------------------------------

/// Locks in: **hitting a cap has to be said out loud.** A truncated list looks exactly like
/// a complete one.
#[tokio::test]
async fn grep_hitting_the_match_cap_says_so_and_spills() {
    let (ctx, store) = tree();
    let (dir, roots) = bench();
    let root = dir.path().canonicalize().unwrap();
    // More matches than the cap, inside one file — the shape of a generated file.
    let content: String = (0..6_000).map(|n| format!("khop {n}\n")).collect();
    std::fs::write(root.join("nhieu.txt"), content).unwrap();

    let outcome = Grep::new(roots, Overflow::new(&ctx))
        .execute(&call("grep", json!({ "pattern": "khop" })))
        .await
        .expect("searches");

    assert!(
        outcome.content.contains("đã dừng ở 5000 khớp"),
        "did not say it hit the cap:\n{}",
        &outcome.content[outcome.content.len().saturating_sub(600)..]
    );
    assert!(
        outcome
            .content
            .contains("thu hẹp bằng `path` hoặc `include`")
            || outcome
                .content
                .contains("Hãy thu hẹp bằng `path` hoặc `include`"),
        "did not say how to narrow the search:\n{}",
        &outcome.content[outcome.content.len().saturating_sub(600)..]
    );

    let search = outcome.meta.get("search").expect("search meta is present");
    assert_eq!(search["total"], json!(5000), "the cap bites at collection");
    assert_eq!(search["truncated"], json!(true));

    let handle = spill_of(&outcome);
    let full = store.read(&handle).expect("the full text is in the store");
    assert!(full.contains("khop 2500"), "the middle must not be lost");
    assert!(outcome.content.len() < full.len() / 4);
}

// --- 4. what is in this directory -------------------------------------------------------

/// Locks four things at once: protected paths are **hidden from the listing** (rule 3),
/// `.gitignore` takes effect **outside a git repo** (`require_git(false)`), hidden files
/// still appear, and the order is directories first then by name.
#[tokio::test]
async fn list_dir_hides_protected_paths_and_honours_gitignore() {
    let (ctx, _) = tree();
    let (dir, roots) = bench();
    let root = dir.path().canonicalize().unwrap();

    std::fs::write(root.join("bi-mat"), "mã thông báo").unwrap();
    std::fs::write(root.join(".gitignore"), "bo-qua/\n").unwrap();
    std::fs::create_dir_all(root.join("bo-qua")).unwrap();
    std::fs::write(root.join("bo-qua/rac.txt"), "rác").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("zeta.txt"), "z").unwrap();
    std::fs::write(root.join("alpha.txt"), "a".repeat(2048)).unwrap();

    let outcome = ListDir::new(roots, Overflow::new(&ctx))
        .execute(&call("list_dir", json!({})))
        .await
        .expect("lists");
    let text = &outcome.content;

    assert!(
        !text.contains("bi-mat"),
        "the listing leaked a protected file:\n{text}"
    );
    // This temp directory is **not** a git repo. Without `require_git(false)` the
    // `.gitignore` is ignored and `bo-qua` shows up.
    assert!(
        !text.contains("bo-qua"),
        "`.gitignore` had no effect outside a git repo:\n{text}"
    );
    assert!(
        text.contains(".gitignore"),
        "hidden files have to appear — they say how the project runs:\n{text}"
    );
    assert!(text.contains("src/"), "directories need a trailing `/`:\n{text}");
    assert!(text.contains("2.0 KB"), "sizes have to be included:\n{text}");

    let dir_at = text.find("src/").expect("src is present");
    let file_at = text.find("alpha.txt").expect("alpha.txt is present");
    assert!(dir_at < file_at, "directories have to come before files:\n{text}");
    assert!(
        text.find("alpha.txt") < text.find("zeta.txt"),
        "files have to be ordered by name:\n{text}"
    );
}

// --- 6. a new tool really registers in the real registry --------------------------------

/// Locks in: **`list_dir` is a real tool in the real tree**, not a struct only the test can
/// call. Goes the exact route the model goes: the registry, the wire name, the pipeline.
#[tokio::test]
async fn list_dir_registers_in_the_real_registry_and_is_callable() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("root canonicalises");
    std::fs::write(root.join("co-that.txt"), "nội dung").unwrap();

    let ctx = Context::root();
    ToolsPlugin
        .apply(&ctx.plugin("tools"))
        .await
        .expect("tools mounts");
    FsPlugin::new([root.clone()], [root.join("bi-mat")])
        .apply(&ctx.plugin("fs"))
        .await
        .expect("fs mounts");

    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().expect("the registry is present");
    let names: Vec<String> = registry
        .schemas(None)
        .into_iter()
        .map(|s| s.name.as_str().to_string())
        .collect();
    assert!(names.contains(&"list_dir".to_string()), "{names:?}");
    assert!(
        names.contains(&"spill_read".to_string()),
        "without `spill_read`, the \"the full text is still there\" message is an empty promise: {names:?}"
    );

    // Resolved by the exact name the model types.
    assert!(matches!(
        registry.resolve(None, "list_dir"),
        Resolution::Found(_, _)
    ));

    let outcome = ToolPipeline::new(&ctx, registry)
        .execute("c1", "list_dir", json!({}))
        .await;
    assert!(!outcome.is_error, "{}", outcome.content);
    assert!(
        outcome.content.contains("co-that.txt"),
        "{}",
        outcome.content
    );
}
