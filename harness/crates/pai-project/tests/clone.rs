//! Blocking hostile URLs, and one real clone.

use std::path::{Path, PathBuf};
use std::process::Command;

use futures::StreamExt;
use pai_project::{CloneEvent, CloneRequest, clone};
use tempfile::TempDir;

fn request(url: &str, parent: &Path) -> CloneRequest {
    CloneRequest {
        url: url.to_string(),
        parent: parent.to_path_buf(),
        name: None,
        depth: None,
    }
}

/// `git clone "ext::sh -c '...'"` runs that command; without this check a pasted URL is a command line.
#[test]
fn a_transport_helper_is_blocked_because_it_is_command_execution() {
    let dir = TempDir::new().expect("temp dir");
    let err = request("ext::sh -c id", dir.path())
        .validate()
        .expect_err("must be blocked");
    assert!(err.to_string().contains("ext"), "the error must say why: {err}");

    // Not just `ext::`: every helper, because the helper list is extensible.
    assert!(request("other::something", dir.path()).validate().is_err());
    // But `::` inside an ordinary URL's path is not a helper.
    assert!(
        request("https://vi.du/a::b.git", dir.path())
            .validate()
            .is_ok(),
        "blocked a legitimate URL by mistake"
    );
}

#[test]
fn a_url_starting_with_a_dash_is_blocked() {
    let dir = TempDir::new().expect("temp dir");
    let err = request("--upload-pack=id", dir.path())
        .validate()
        .expect_err("must be blocked");
    assert!(err.to_string().contains('-'), "the error must say why: {err}");
}

#[test]
fn unexpected_schemes_are_blocked() {
    let dir = TempDir::new().expect("temp dir");
    for url in ["ftp://vi.du/x.git", "javascript://x", "/home/repo", ""] {
        assert!(
            request(url, dir.path()).validate().is_err(),
            "`{url}` slipped through"
        );
    }
    for url in [
        "https://vi.du/x.git",
        "http://vi.du/x.git",
        "ssh://git@vi.du/x.git",
        "git://vi.du/x.git",
        "file:///home/repo",
        "git@vi.du:group/x.git",
    ] {
        assert!(
            request(url, dir.path()).validate().is_ok(),
            "`{url}` was blocked wrongly"
        );
    }
}

#[test]
fn a_name_escaping_the_parent_directory_is_blocked() {
    let dir = TempDir::new().expect("temp dir");
    for name in ["..", "../outside", "a/b", "a\\b", ""] {
        let mut req = request("https://vi.du/x.git", dir.path());
        req.name = Some(name.to_string());
        assert!(req.validate().is_err(), "name `{name}` slipped through");
        assert!(
            req.destination().is_err(),
            "name `{name}` still produced a destination"
        );
    }

    let mut req = request("https://vi.du/x.git", dir.path());
    req.name = Some("elsewhere".to_string());
    assert_eq!(
        req.destination().expect("a valid name"),
        dir.path().join("elsewhere")
    );
    // With no name given it is derived from the URL, and a trailing `.git` is dropped.
    assert_eq!(
        request("https://vi.du/group/x.git", dir.path())
            .destination()
            .expect("the name is derivable"),
        dir.path().join("x")
    );
}

#[test]
fn a_destination_holding_data_is_never_cloned_over() {
    let dir = TempDir::new().expect("temp dir");
    let destination = dir.path().join("x");
    std::fs::create_dir(&destination).expect("create");
    std::fs::write(destination.join("cua-toi.txt"), "còn").expect("write");

    let err = request("https://vi.du/x.git", dir.path())
        .validate()
        .expect_err("must be blocked");
    assert!(err.to_string().contains("mất dữ liệu"), "vague error: {err}");

    // An empty directory is fine: that is the folder the user just made to clone into.
    std::fs::remove_file(destination.join("cua-toi.txt")).expect("remove");
    assert!(
        request("https://vi.du/x.git", dir.path())
            .validate()
            .is_ok()
    );
}

fn git(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Build a source repo with exactly one commit and return its `file://` URL.
fn source_repo(root: &Path) -> Option<String> {
    let source = root.join("source");
    std::fs::create_dir(&source).ok()?;
    if !git(&source, &["init", "-q"]) {
        return None;
    }
    std::fs::write(source.join("xin-chao.txt"), "xin chào").ok()?;
    if !git(&source, &["add", "."]) {
        return None;
    }
    // A CI machine may have no identity configured; set it inline so the commit asks nothing.
    let committed = git(
        &source,
        &[
            "-c",
            "user.email=test@vi.du",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "first",
        ],
    );
    if !committed {
        return None;
    }
    let real = source.canonicalize().ok()?;
    Some(format!("file://{}", real.display()))
}

#[tokio::test]
async fn a_real_clone_emits_progress_and_ends_with_done() {
    if !has_git() {
        eprintln!("skipped: no `git` on PATH");
        return;
    }
    let dir = TempDir::new().expect("temp dir");
    let Some(url) = source_repo(dir.path()) else {
        eprintln!("skipped: could not build a source repo with `git`");
        return;
    };
    let parent = dir.path().join("dest");
    std::fs::create_dir(&parent).expect("create the containing directory");

    let mut stream = clone(CloneRequest {
        url,
        parent: parent.clone(),
        name: Some("copy".to_string()),
        depth: None,
    });

    let mut saw_tick = false;
    let mut finished: Option<PathBuf> = None;
    while let Some(event) = stream.next().await {
        match event {
            CloneEvent::Phase { .. } | CloneEvent::Progress { .. } => saw_tick = true,
            CloneEvent::Line { .. } => {}
            CloneEvent::Done { path } => finished = Some(path),
            CloneEvent::Failed { message } => panic!("clone failed: {message}"),
        }
    }

    assert!(saw_tick, "the stream emitted no ticks — the UI would sit still");
    let path = finished.expect("must end with Done");
    assert_eq!(path, parent.join("copy"));
    assert!(
        path.join("xin-chao.txt").exists(),
        "the clone finished but the file is not there"
    );
}

/// A blocked URL must end the stream with `Failed`: silence looks exactly like a slow clone.
#[tokio::test]
async fn a_bad_url_ends_the_stream_with_failed_rather_than_hanging() {
    let dir = TempDir::new().expect("temp dir");
    let mut stream = clone(request("ext::sh -c id", dir.path()));
    let first = stream.next().await.expect("there must be an event");
    assert!(matches!(first, CloneEvent::Failed { .. }), "{first:?}");
    assert!(
        stream.next().await.is_none(),
        "once failed, the stream must close"
    );
}
