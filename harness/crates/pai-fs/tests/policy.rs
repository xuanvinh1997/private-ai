//! Invariants whose loss means losing the user's files. Each test locks a documented
//! sentence: a red test means either the code is wrong or that sentence no longer holds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pai_core::Context;
use pai_fs::path::FileRoots;
use pai_fs::provider::{FsProvider, LocalFs};
use pai_fs::tools::{edit::Edit, glob::GlobTool, grep::Grep, read::Read, write::Write};
use pai_fs::{ReadLedger, looks_binary};
use pai_tools::{Invocation, Overflow, Tool, ToolName, ToolOutcome};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

/// A budget with no spill store, so nothing folds here; folding is tested in `budget.rs`.
fn no_budget() -> Overflow {
    Overflow::new(&Context::root())
}

fn call(name: &str, args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(name), "c1", map)
}

fn bench() -> (TempDir, FileRoots, Arc<dyn FsProvider>, Arc<ReadLedger>) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("root canonicalises");
    let roots = FileRoots::new([root.clone()], [root.join("bi-mat")]);
    (
        dir,
        roots,
        Arc::new(LocalFs),
        Arc::new(ReadLedger::default()),
    )
}

async fn read_ok(read: &Read, path: &Path) -> ToolOutcome {
    read.execute(&call(
        "read",
        json!({ "file_path": path.display().to_string() }),
    ))
    .await
    .expect("reads")
}

#[tokio::test]
async fn dot_dot_and_symlinks_cannot_escape_the_root() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("ngoai.txt");
    std::fs::write(&secret, "không được đọc").unwrap();

    let read = Read::new(fs, roots, ledger, no_budget());

    // Climbing out with `..`.
    let escape = root.join("..").join(secret.file_name().unwrap());
    let err = read
        .execute(&call(
            "read",
            json!({ "file_path": escape.display().to_string() }),
        ))
        .await;
    assert!(err.is_err(), "`..` must not escape the root");

    // Going out through a symlink that lives inside the root.
    #[cfg(unix)]
    {
        let link = root.join("loi-tat.txt");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let err = read
            .execute(&call(
                "read",
                json!({ "file_path": link.display().to_string() }),
            ))
            .await;
        assert!(err.is_err(), "a symlink pointing outside must count as outside");
    }
}

#[tokio::test]
async fn a_protected_path_is_unreadable_and_absent_from_listings() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let secret = root.join("bi-mat");
    std::fs::write(&secret, "mã thông báo").unwrap();
    std::fs::write(root.join("thuong.txt"), "bình thường").unwrap();

    let read = Read::new(fs, roots.clone(), ledger, no_budget());
    let err = read
        .execute(&call(
            "read",
            json!({ "file_path": secret.display().to_string() }),
        ))
        .await
        .expect_err("a protected file cannot be read");
    assert!(
        err.to_string().contains("được bảo vệ"),
        "the reason has to be the right one: {err}"
    );

    // Nor may it leak through a listing: naming the file already reveals it exists.
    let listing = GlobTool::new(roots)
        .execute(&call("glob", json!({ "pattern": "*" })))
        .await
        .expect("lists");
    assert!(
        !listing.content.contains("bi-mat"),
        "the listing leaked a protected file:\n{}",
        listing.content
    );
    assert!(listing.content.contains("thuong.txt"));
}

#[tokio::test]
async fn a_binary_file_is_refused_rather_than_returned_as_garbage() {
    assert!(looks_binary(&[0x7f, b'E', b'L', b'F', 0x00]));
    assert!(!looks_binary("chào bạn".as_bytes()));

    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let binary = root.join("a.bin");
    std::fs::write(&binary, [0x00, 0x01, 0x02]).unwrap();

    let err = Read::new(fs, roots, ledger, no_budget())
        .execute(&call(
            "read",
            json!({ "file_path": binary.display().to_string() }),
        ))
        .await
        .expect_err("a binary file is refused");
    assert!(err.to_string().contains("nhị phân"), "{err}");
}

#[tokio::test]
async fn a_multi_match_edit_errors_and_changes_nothing() {
    let (dir, roots, fs, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("a.rs");
    let before = "let x = 1;\nlet x = 1;\n";
    std::fs::write(&file, before).unwrap();

    let edit = Edit::new(fs, roots);
    let err = edit
        .execute(&call(
            "edit",
            json!({
                "file_path": file.display().to_string(),
                "old_string": "let x = 1;",
                "new_string": "let x = 2;",
            }),
        ))
        .await
        .expect_err("two matches must be refused");
    assert!(
        err.to_string().contains('2'),
        "the error has to say how many places matched: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        before,
        "the file has to be untouched"
    );

    // Stating the intent explicitly makes it work.
    edit.execute(&call(
        "edit",
        json!({
            "file_path": file.display().to_string(),
            "old_string": "let x = 1;",
            "new_string": "let x = 2;",
            "replace_all": true,
        }),
    ))
    .await
    .expect("replace_all edits");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "let x = 2;\nlet x = 2;\n"
    );
}

#[tokio::test]
async fn hunk_line_numbers_are_the_real_line_numbers_in_the_file() {
    let (dir, roots, fs, ledger) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("a.txt");
    // The change sits on line 12, far enough to tell hunk-relative from file-relative counting.
    let before: String = (1..=20).map(|n| format!("dòng {n}\n")).collect();
    std::fs::write(&file, &before).unwrap();

    let read = Read::new(fs.clone(), roots.clone(), ledger, no_budget());
    read_ok(&read, &file).await;

    let outcome = Edit::new(fs, roots)
        .execute(&call(
            "edit",
            json!({
                "file_path": file.display().to_string(),
                "old_string": "dòng 12",
                "new_string": "dòng mười hai",
            }),
        ))
        .await
        .expect("edits");

    let diffs = outcome
        .meta
        .get("diffs")
        .and_then(|v| v.as_array())
        .expect("diffs are present");
    let hunk = diffs.first().expect("one hunk");
    // Three lines of context, so the hunk starts at line 9.
    assert_eq!(
        hunk["old_start"],
        json!(9),
        "the hunk has to carry its real position: {hunk}"
    );
    assert_eq!(hunk["new_start"], json!(9));
}

#[tokio::test]
async fn writing_a_new_file_gives_old_text_null_not_an_empty_string() {
    let (dir, roots, fs, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    let file = root.join("moi.txt");

    let outcome = Write::new(fs, roots)
        .execute(&call(
            "write",
            json!({ "file_path": file.display().to_string(), "content": "xin chào\n" }),
        ))
        .await
        .expect("creates");

    let diffs = outcome
        .meta
        .get("diffs")
        .and_then(|v| v.as_array())
        .expect("diffs are present");
    // `null` means new file, an empty string means an empty old file; the UI draws them apart.
    assert_eq!(diffs[0]["old_text"], Value::Null);
}

#[tokio::test]
async fn grep_skips_binary_files_and_still_counts_the_total() {
    let (dir, roots, _, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("a.txt"), "cần tìm\nkhông\ncần tìm\n").unwrap();
    let mut binary = vec![0u8];
    binary.extend_from_slice("cần tìm".as_bytes());
    binary.push(0);
    std::fs::write(root.join("b.bin"), binary).unwrap();

    let outcome = Grep::new(roots, no_budget())
        .execute(&call("grep", json!({ "pattern": "cần tìm" })))
        .await
        .expect("searches");

    let search = outcome.meta.get("search").expect("search is present");
    assert_eq!(
        search["total"],
        json!(2),
        "a binary file must not contribute matches"
    );
    assert_eq!(search["shape"], json!("matches"));
    assert!(!outcome.content.contains("b.bin"));
}

#[tokio::test]
async fn glob_lists_no_directories_and_a_slashless_pattern_matches_at_any_depth() {
    let (dir, roots, _, _) = bench();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src/sau")).unwrap();
    std::fs::write(root.join("src/a.rs"), "").unwrap();
    std::fs::write(root.join("src/sau/b.rs"), "").unwrap();

    let outcome = GlobTool::new(roots)
        .execute(&call("glob", json!({ "pattern": "*.rs" })))
        .await
        .expect("searches");

    // Without match-at-any-depth this is empty and the model concludes there are no Rust files.
    assert!(outcome.content.contains("a.rs"));
    assert!(
        outcome.content.contains("b.rs"),
        "a pattern without `/` has to match at any depth"
    );
    assert!(
        !outcome.content.contains("src\n"),
        "directories must not be listed"
    );
}

#[test]
fn empty_roots_means_refuse_everything_not_allow_everything() {
    let roots = FileRoots::new(Vec::<PathBuf>::new(), Vec::<PathBuf>::new());
    assert!(roots.resolve_read(Path::new("/etc/hosts")).is_err());
}
