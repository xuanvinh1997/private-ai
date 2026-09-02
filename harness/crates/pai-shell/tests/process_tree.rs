//! Invariants of running a command.
//!
//! The most important test here is the one about grandchildren. Killing a shell without
//! killing its descendants is the kind of bug that never announces itself — everything
//! still looks like it works, except a port is held, a file lock is held, and the next turn
//! runs on a contaminated machine.

use std::path::PathBuf;
use std::time::Duration;

use pai_core::Context;
use pai_sandbox::Policy;
use pai_shell::provider::{LocalShell, Request, ShellExecutor};
use tokio_util::sync::CancellationToken;

/// A shell with no confinement. The tests below check the process tree, not the sandbox;
/// wrapping another `sandbox-exec` layer around them only makes them measure something
/// else.
fn shell() -> LocalShell {
    LocalShell::new(Context::root(), Policy::danger_full_access("/tmp"))
}

fn request(command: &str, timeout: Option<Duration>, cancel: CancellationToken) -> Request {
    Request {
        command: command.to_string(),
        cwd: PathBuf::from("/tmp"),
        timeout,
        cancel,
    }
}

#[tokio::test]
async fn the_exit_code_survives_the_round_trip() {
    let shell = shell();
    let ok = shell
        .run(request("echo xin-chao", None, CancellationToken::new()))
        .await
        .expect("runs");
    assert_eq!(ok.exit_code, Some(0));
    assert!(ok.output.contains("xin-chao"));

    let failed = shell
        .run(request("exit 101", None, CancellationToken::new()))
        .await
        .expect("runs");
    // A non-zero exit is still a successful run: the command did exactly what it was told.
    assert_eq!(failed.exit_code, Some(101));
}

#[tokio::test]
async fn stdout_and_stderr_interleave_in_arrival_order() {
    let run = shell()
        .run(request(
            "echo mot; echo hai >&2; echo ba",
            None,
            CancellationToken::new(),
        ))
        .await
        .expect("runs");
    for line in ["mot", "hai", "ba"] {
        assert!(
            run.output.contains(line),
            "missing `{line}` in:\n{}",
            run.output
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn killing_a_command_kills_its_grandchildren() {
    use std::path::Path;

    let marker = std::env::temp_dir().join(format!("pai-grandchild-{}", uuid_like()));
    let _ = std::fs::remove_file(&marker);

    // The grandchild sleeps 30 seconds and only then touches the marker file. If it
    // survives the kill the file appears; if it dies with its parent, it never does.
    let command = format!("(sleep 30; touch {}) & sleep 30", marker.display());
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        token.cancel();
    });

    let run = shell()
        .run(request(&command, None, cancel))
        .await
        .expect("runs");
    assert_eq!(run.interrupted.as_deref(), Some("lượt đã bị huỷ"));

    // Wait past the point where the grandchild meant to touch the file. It must not.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !Path::new(&marker).exists(),
        "the grandchild survived the kill: {}",
        marker.display()
    );
}

#[tokio::test]
async fn a_timeout_stops_the_command_and_still_returns_what_it_printed() {
    let run = shell()
        .run(request(
            "echo bat-dau; sleep 30",
            Some(Duration::from_millis(400)),
            CancellationToken::new(),
        ))
        .await
        .expect("runs");

    assert!(
        run.interrupted.is_some(),
        "a timeout has to be reported, not swallowed"
    );
    // What was printed before the stop is still useful and must not be thrown away.
    assert!(
        run.output.contains("bat-dau"),
        "lost output that had already arrived:\n{}",
        run.output
    );
    assert_eq!(run.exit_code, None);
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}
