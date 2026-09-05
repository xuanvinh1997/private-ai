//! End-to-end over a real, tiny repository built in a temp directory.
//!
//! No network and no fixture checked into the tree: every test here does `git init`, writes
//! files and commits them, which is also the only way to be sure the parsers survive what a
//! real `git` prints rather than what we imagined it prints.
//!
//! On a machine without `git` the whole file skips itself out loud. A red test there would
//! be a lie — nothing is broken, the tool simply cannot run.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use pai_core::Context;
use pai_git::tools::{GitBlame, GitDiff, GitLog, GitShow, GitStatus};
use pai_git::repo::Repo;
use pai_tools::{Invocation, Overflow, Tool, ToolError, ToolName, ToolOutcome};
use serde_json::{Value, json};
use std::sync::Arc;

/// Skip rather than fail when the binary this crate is built around is absent.
fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

macro_rules! need_git {
    () => {
        if !has_git() {
            eprintln!("bỏ qua: máy chạy test không có `git` trong PATH");
            return;
        }
    };
}

/// A throwaway repository with a fixed author and fixed dates, so assertions can name them.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let fixture = Fixture {
            dir: tempfile::tempdir().expect("tạo được thư mục tạm"),
        };
        fixture.git(&["init", "-q"]);
        // Not `init -b main`: that flag is younger than some git versions still in the wild.
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
        fixture
    }

    fn root(&self) -> PathBuf {
        // macOS puts temp directories behind a `/private` symlink; git reports the resolved
        // path, and a test comparing the two would fail for no interesting reason.
        self.dir
            .path()
            .canonicalize()
            .unwrap_or_else(|_| self.dir.path().to_path_buf())
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(self.dir.path())
            // Cut the machine's own git configuration out entirely: a global `gpgsign`, a
            // template directory or a hooks path would otherwise decide whether these pass.
            .env("HOME", self.dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Ai Đó")
            .env("GIT_AUTHOR_EMAIL", "ai@vidu.vn")
            .env("GIT_COMMITTER_NAME", "Ai Đó")
            .env("GIT_COMMITTER_EMAIL", "ai@vidu.vn")
            .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05+07:00")
            .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05+07:00")
            .args(args)
            .output()
            .expect("chạy được `git`");
        assert!(
            output.status.success(),
            "`git {}` thất bại: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(&self, name: &str, body: &str) {
        let path = self.dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("tạo được thư mục cha");
        }
        std::fs::write(path, body).expect("ghi được tệp");
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    fn repo(&self) -> Arc<Repo> {
        Arc::new(Repo::new(self.root()))
    }
}

fn overflow() -> Overflow {
    Overflow::new(&Context::root())
}

fn invocation(name: &str, args: Value) -> Invocation {
    let arguments = match args {
        Value::Object(map) => map,
        _ => panic!("tham số phải là một object"),
    };
    Invocation::new(ToolName::new(name), "test", arguments)
}

async fn run(tool: &dyn Tool, name: &str, args: Value) -> ToolOutcome {
    let call = invocation(name, args);
    tool.execute(&call).await.expect("tool chạy được")
}

async fn fail(tool: &dyn Tool, name: &str, args: Value) -> ToolError {
    let call = invocation(name, args);
    tool.execute(&call)
        .await
        .expect_err("tool phải báo lỗi ở đây")
}

/// Line-numbered content, so a diff of it is predictably long.
fn numbered(lines: usize, tag: &str) -> String {
    (1..=lines)
        .map(|n| format!("dòng {n} {tag}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn status_groups_staged_unstaged_and_untracked() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    fixture.write("a.txt", "một\nhai\n");
    fixture.write("da_stage.txt", "nội dung\n");
    fixture.git(&["add", "da_stage.txt"]);
    fixture.write("chua_theo_doi.txt", "mới\n");

    let tool = GitStatus::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitStatus::NAME, json!({})).await;

    assert!(!outcome.is_error);
    assert!(outcome.content.contains("Nhánh: main"), "{}", outcome.content);
    assert!(outcome.content.contains("Đã đưa vào chỉ mục"), "{}", outcome.content);
    assert!(outcome.content.contains("da_stage.txt"), "{}", outcome.content);
    assert!(outcome.content.contains("Đã sửa nhưng chưa đưa vào chỉ mục"), "{}", outcome.content);
    assert!(outcome.content.contains("a.txt"), "{}", outcome.content);
    assert!(outcome.content.contains("Chưa được git theo dõi"), "{}", outcome.content);
    assert!(outcome.content.contains("chua_theo_doi.txt"), "{}", outcome.content);

    let structured = outcome.structured.expect("có structured");
    assert_eq!(structured["shape"], "git.status");
    assert_eq!(structured["total"], 3);
}

#[tokio::test]
async fn status_says_a_clean_tree_is_clean() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitStatus::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitStatus::NAME, json!({})).await;
    assert!(outcome.content.contains("Cây làm việc sạch"), "{}", outcome.content);
}

#[tokio::test]
async fn status_caps_the_listing_and_says_so() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("goc.txt", "gốc\n");
    fixture.commit("commit đầu");
    for n in 0..12 {
        fixture.write(&format!("f{n}.txt"), "x\n");
    }

    let tool = GitStatus::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitStatus::NAME, json!({ "max_entries": 5 })).await;
    assert!(outcome.content.contains("còn 7 mục nữa không liệt kê"), "{}", outcome.content);
    assert!(outcome.content.contains("tổng 12"), "{}", outcome.content);
}

#[tokio::test]
async fn log_reads_commits_newest_first_and_honours_max_count() {
    need_git!();
    let fixture = Fixture::new();
    for n in 1..=3 {
        fixture.write("a.txt", &format!("lần {n}\n"));
        fixture.commit(&format!("commit số {n}"));
    }

    let tool = GitLog::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitLog::NAME, json!({ "max_count": 2 })).await;

    let structured = outcome.structured.expect("có structured");
    let commits = structured["commits"].as_array().expect("mảng commit");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "commit số 3");
    assert_eq!(commits[1]["subject"], "commit số 2");
    assert_eq!(commits[0]["author"], "Ai Đó");
    assert_eq!(commits[0]["hash"].as_str().expect("sha").len(), 40);
    assert!(outcome.content.contains("commit số 3"), "{}", outcome.content);
}

#[tokio::test]
async fn log_keeps_a_multi_line_body_and_lists_files_on_request() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.write("b.txt", "hai\n");
    fixture.commit("Chủ đề\n\nThân bài dòng một\nThân bài dòng hai");

    let tool = GitLog::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitLog::NAME, json!({ "files": true })).await;

    let structured = outcome.structured.expect("có structured");
    let commit = &structured["commits"][0];
    assert_eq!(commit["subject"], "Chủ đề");
    assert!(
        commit["body"].as_str().expect("body").contains("Thân bài dòng hai"),
        "{commit}"
    );
    let files = commit["files"].as_array().expect("mảng tệp");
    assert_eq!(files.len(), 2, "{commit}");
}

#[tokio::test]
async fn log_filters_by_path() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("chạm a");
    fixture.write("b.txt", "hai\n");
    fixture.commit("chạm b");

    let tool = GitLog::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitLog::NAME, json!({ "paths": ["b.txt"] })).await;
    let structured = outcome.structured.expect("có structured");
    let commits = structured["commits"].as_array().expect("mảng commit");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["subject"], "chạm b");
}

#[tokio::test]
async fn diff_shows_the_working_tree_change() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");
    fixture.write("a.txt", "một\nhai\n");

    let tool = GitDiff::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitDiff::NAME, json!({})).await;
    assert!(outcome.content.contains("+hai"), "{}", outcome.content);
    assert!(outcome.content.contains("a/a.txt"), "{}", outcome.content);
}

#[tokio::test]
async fn diff_announces_what_it_cut() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", &numbered(60, "cũ"));
    fixture.commit("commit đầu");
    fixture.write("a.txt", &numbered(60, "mới"));

    let tool = GitDiff::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitDiff::NAME, json!({ "max_lines": 6 })).await;

    assert!(outcome.content.contains("đã cắt"), "{}", outcome.content);
    assert!(outcome.content.contains("dòng cuối trên tổng"), "{}", outcome.content);
    assert_eq!(outcome.structured.expect("có structured")["truncated"], true);
}

#[tokio::test]
async fn diff_reports_an_empty_result_with_its_subject() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitDiff::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitDiff::NAME, json!({})).await;
    assert!(outcome.content.contains("Không có khác biệt nào"), "{}", outcome.content);
    assert!(outcome.content.contains("cây làm việc"), "{}", outcome.content);
}

#[tokio::test]
async fn diff_between_two_revisions_and_stat_only() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");
    fixture.write("a.txt", "một\nhai\n");
    fixture.commit("commit hai");

    let tool = GitDiff::new(fixture.repo(), overflow());
    let outcome = run(
        &tool,
        GitDiff::NAME,
        json!({ "base": "HEAD~1", "head": "HEAD", "stat_only": true }),
    )
    .await;
    assert!(outcome.content.contains("a.txt"), "{}", outcome.content);
    // `--stat` summarises; the added line itself must not be there.
    assert!(!outcome.content.contains("+hai"), "{}", outcome.content);
}

#[tokio::test]
async fn show_renders_message_and_diff() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");
    fixture.write("a.txt", "một\nhai\n");
    fixture.commit("Thêm dòng hai");

    let tool = GitShow::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitShow::NAME, json!({})).await;
    assert!(outcome.content.contains("Thêm dòng hai"), "{}", outcome.content);
    assert!(outcome.content.contains("+hai"), "{}", outcome.content);
    assert_eq!(outcome.structured.expect("có structured")["rev"], "HEAD");
}

#[tokio::test]
async fn blame_attributes_each_line() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\nhai\nba\n");
    fixture.commit("commit đầu");

    let tool = GitBlame::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitBlame::NAME, json!({ "file": "a.txt" })).await;

    let structured = outcome.structured.expect("có structured");
    let lines = structured["lines"].as_array().expect("mảng dòng");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["number"], 1);
    assert_eq!(lines[0]["text"], "một");
    assert_eq!(lines[0]["author"], "Ai Đó");
    assert_eq!(lines[0]["date"], "2024-01-02");
    assert!(outcome.content.contains("a.txt — dòng 1..3"), "{}", outcome.content);
}

#[tokio::test]
async fn blame_windows_and_says_the_window_filled() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", &numbered(30, "x"));
    fixture.commit("commit đầu");

    let tool = GitBlame::new(fixture.repo(), overflow());
    let outcome = run(
        &tool,
        GitBlame::NAME,
        json!({ "file": "a.txt", "start": 5, "limit": 3 }),
    )
    .await;
    let structured = outcome.structured.expect("có structured");
    let lines = structured["lines"].as_array().expect("mảng dòng");
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["number"], 5);
    assert!(outcome.content.contains("`start: 8`"), "{}", outcome.content);
}

/// Point `.gitattributes` at a `textconv` driver and return the marker it would print.
///
/// A textconv driver is a shell command git runs on the content of a file before comparing or
/// blaming it. The attribute that selects it lives *inside the repository*, so it is a file a
/// contributor writes. Unix only: the driver is run through `sh`.
#[cfg(unix)]
fn arm_textconv(fixture: &Fixture) -> &'static str {
    fixture.write(".gitattributes", "*.bin diff=bay\n");
    fixture.git(&["config", "diff.bay.textconv", "echo DA-CHAY-TEXTCONV"]);
    "DA-CHAY-TEXTCONV"
}

#[cfg(unix)]
#[tokio::test]
async fn diff_does_not_run_a_textconv_driver() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("s.bin", "cũ\n");
    fixture.commit("commit đầu");
    let marker = arm_textconv(&fixture);
    fixture.write("s.bin", "mới\n");

    let tool = GitDiff::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitDiff::NAME, json!({})).await;
    // `--no-ext-diff` alone does not stop this: without `--no-textconv` the command runs, and
    // the diff of a changed file comes back *empty* because both sides converted to one text.
    assert!(!outcome.content.contains(marker), "{}", outcome.content);
    assert!(outcome.content.contains("+mới"), "{}", outcome.content);
}

#[cfg(unix)]
#[tokio::test]
async fn blame_does_not_run_a_textconv_driver() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("s.bin", "nội dung thật\n");
    fixture.commit("commit đầu");
    let marker = arm_textconv(&fixture);

    let tool = GitBlame::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitBlame::NAME, json!({ "file": "s.bin" })).await;
    assert!(!outcome.content.contains(marker), "{}", outcome.content);
    assert!(outcome.content.contains("nội dung thật"), "{}", outcome.content);
}

#[tokio::test]
async fn status_names_both_sides_of_a_rename() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("cu.txt", "nội dung đủ dài để git nhận ra là cùng một tệp\n");
    fixture.commit("commit đầu");
    fixture.git(&["mv", "cu.txt", "moi.txt"]);

    let tool = GitStatus::new(fixture.repo(), overflow());
    let outcome = run(&tool, GitStatus::NAME, json!({})).await;

    let entry = &outcome.structured.expect("có structured")["entries"][0];
    // The new name alone in `path`, not `cu.txt -> moi.txt`: what is in `path` gets passed
    // back to other tools as a path.
    assert_eq!(entry["path"], "moi.txt", "{entry}");
    assert_eq!(entry["orig"], "cu.txt", "{entry}");
    assert!(outcome.content.contains("đổi tên từ cu.txt"), "{}", outcome.content);
}

#[tokio::test]
async fn log_drops_whole_commits_and_says_how_many() {
    need_git!();
    let fixture = Fixture::new();
    for n in 1..=4 {
        fixture.write("a.txt", &format!("lần {n}\n"));
        fixture.commit(&format!("commit số {n}"));
    }

    let tool = GitLog::new(fixture.repo(), overflow());
    // Three lines per commit, so this budget holds one and a bit: the "bit" must not appear.
    let outcome = run(&tool, GitLog::NAME, json!({ "max_lines": 4 })).await;

    assert!(outcome.content.contains("còn 3 commit nữa không hiện"), "{}", outcome.content);
    assert!(outcome.content.contains("commit số 4"), "{}", outcome.content);
    assert!(!outcome.content.contains("commit số 3"), "{}", outcome.content);

    let structured = outcome.structured.expect("có structured");
    // `structured` is forwarded to the model as MCP `structured_content`, so it must list
    // exactly the commits the text shows — no more.
    assert_eq!(structured["commits"].as_array().map(Vec::len), Some(1));
    assert_eq!(structured["total"], 4);
    assert_eq!(structured["truncated"], true);
}

#[tokio::test]
async fn blame_past_the_end_of_the_file_is_an_argument_error() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\nhai\n");
    fixture.commit("commit đầu");

    let tool = GitBlame::new(fixture.repo(), overflow());
    let err = fail(&tool, GitBlame::NAME, json!({ "file": "a.txt", "start": 50 })).await;
    // git makes this fatal rather than clamping; as `Failed` the model reads it as a broken
    // machine and retries the same call.
    assert!(matches!(err, ToolError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("`start` nhỏ hơn"), "{err}");
}

#[tokio::test]
async fn diff_refuses_a_head_with_no_base() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitDiff::new(fixture.repo(), overflow());
    // Silently answering the working-tree question here would look like a successful answer
    // about `HEAD~1`, which is the one failure mode nobody catches.
    let err = fail(&tool, GitDiff::NAME, json!({ "head": "HEAD~1" })).await;
    assert!(matches!(err, ToolError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("không có `base`"), "{err}");
}

#[tokio::test]
async fn a_path_outside_the_repo_is_refused_before_git_runs() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitBlame::new(fixture.repo(), overflow());
    let err = fail(
        &tool,
        GitBlame::NAME,
        json!({ "file": "../../../etc/passwd" }),
    )
    .await;
    // `Invalid`, not `Failed`: this is an argument the model can correct.
    assert!(matches!(err, ToolError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("nằm ngoài kho git"), "{err}");
}

#[tokio::test]
async fn a_revision_that_is_really_an_option_is_refused() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitShow::new(fixture.repo(), overflow());
    let err = fail(&tool, GitShow::NAME, json!({ "rev": "--output=/tmp/x" })).await;
    assert!(matches!(err, ToolError::Invalid(_)), "{err:?}");
    assert!(err.to_string().contains("tuỳ chọn dòng lệnh"), "{err}");
}

#[tokio::test]
async fn a_directory_that_is_not_a_repository_says_so() {
    need_git!();
    let dir = tempfile::tempdir().expect("tạo được thư mục tạm");
    let repo = Arc::new(Repo::new(dir.path().to_path_buf()));

    let tool = GitStatus::new(repo, overflow());
    let err = fail(&tool, GitStatus::NAME, json!({})).await;
    assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
    assert!(err.to_string().contains("không phải là một kho git"), "{err}");
}

#[tokio::test]
async fn a_cancelled_call_does_not_return_a_result() {
    need_git!();
    let fixture = Fixture::new();
    fixture.write("a.txt", "một\n");
    fixture.commit("commit đầu");

    let tool = GitLog::new(fixture.repo(), overflow());
    let call = invocation(GitLog::NAME, json!({}));
    // The token is shared with the clone the tool takes, so cancelling here is exactly what
    // a timeout does to a call already in flight.
    call.cancel_token().cancel();

    let err = tool.execute(&call).await.expect_err("phải bị huỷ");
    assert!(err.to_string().contains("bị huỷ"), "{err}");
}

#[tokio::test]
async fn every_tool_declares_itself_read_only_and_untrusted() {
    let fixture_root = Path::new("/khong-ton-tai");
    let repo = Arc::new(Repo::new(fixture_root.to_path_buf()));
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(GitStatus::new(repo.clone(), overflow())),
        Box::new(GitDiff::new(repo.clone(), overflow())),
        Box::new(GitLog::new(repo.clone(), overflow())),
        Box::new(GitShow::new(repo.clone(), overflow())),
        Box::new(GitBlame::new(repo, overflow())),
    ];
    for tool in tools {
        let meta = tool.meta();
        assert!(!meta.mutating, "{} không được là mutating", tool.schema().name);
        assert!(!meta.leaves_device, "{} không gửi gì ra khỏi máy", tool.schema().name);
        assert!(meta.returns_untrusted_content, "{} phải là untrusted", tool.schema().name);
        assert!(meta.concurrency_safe, "{} chạy song song được", tool.schema().name);
        // The description the model reads must carry the untrusted warning.
        let framed = meta.frame(&tool.schema().description);
        assert!(framed.contains("không đáng tin cậy"), "{framed}");
    }
}
