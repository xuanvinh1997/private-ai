//! Is the Linux confinement real. Like the macOS tests, these run real commands rather than
//! compare argument strings. They need a kernel with Landlock and no seccomp filter over the
//! syscall; where it is blocked, the provider must report `None` with a reason.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Command, Stdio};

use pai_sandbox::landlock::Landlock;
use pai_sandbox::seam::{Enforcement, SandboxProvider};
use pai_sandbox::{Mode, Policy};
use tempfile::TempDir;

/// The helper binary, built by this test suite itself.
const RUNNER: &str = env!("CARGO_BIN_EXE_pai-landlock-run");

fn provider() -> Landlock {
    Landlock::with_runner(RUNNER)
}

/// Can this kernel confine; if not the test skips rather than going red on the environment.
fn can_confine() -> bool {
    match provider().enforcement() {
        Enforcement::None(reason) => {
            eprintln!("skipped: {reason}");
            false
        }
        _ => true,
    }
}

fn runs(policy: &Policy, command: &str) -> bool {
    let argv = provider()
        .wrap(vec!["/bin/sh".into(), "-c".into(), command.into()], policy)
        .expect("argv wraps");
    let (program, args) = argv.split_first().expect("argv is not empty");
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn workspace_write_allows_writes_inside_the_workspace() {
    if !can_confine() {
        return;
    }
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().canonicalize().expect("canonicalises");
    let policy = Policy::workspace_write(&root);
    assert!(
        runs(
            &policy,
            &format!("echo xin-chao > {}/a.txt", root.display())
        ),
        "writing inside the workspace has to work, or the agent cannot edit the repo"
    );
}

#[test]
fn workspace_write_blocks_writes_outside_the_workspace() {
    if !can_confine() {
        return;
    }
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().canonicalize().expect("canonicalises");
    let policy = Policy::workspace_write(&root);

    // Not a `TempDir` or `/var/tmp` as the outside location: both are deliberately writable.
    let home = std::env::var("HOME").expect("HOME is set");
    let target = Path::new(&home).join(format!("pai-must-not-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&target);

    assert!(
        !runs(&policy, &format!("echo x > {}", target.display())),
        "writing outside the workspace has to fail"
    );
    let leaked = target.exists();
    let _ = std::fs::remove_file(&target);
    assert!(
        !leaked,
        "the write was blocked but the file appeared anyway: {}",
        target.display()
    );
}

#[test]
fn read_only_blocks_writes_even_inside_the_workspace() {
    if !can_confine() {
        return;
    }
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().canonicalize().expect("canonicalises");
    let policy = Policy::read_only(&root);

    assert!(!runs(
        &policy,
        &format!("echo x > {}/a.txt", root.display())
    ));
    // Reading must still work, and this command also exercises the `/dev/null` hole.
    assert!(runs(&policy, "/bin/ls / > /dev/null"));
}

#[test]
fn danger_full_access_wraps_nothing() {
    let workspace = TempDir::new().expect("temp dir");
    let policy = Policy::danger_full_access(workspace.path());
    let argv = vec!["/bin/echo".to_string(), "hello".to_string()];

    // The absence of a sandbox, so nothing is wrapped; this never touches the kernel.
    assert_eq!(
        provider().wrap(argv.clone(), &policy).expect("wraps"),
        argv
    );
    assert_eq!(policy.mode, Mode::DangerFullAccess);
}

#[test]
fn without_confinement_it_refuses_to_run_rather_than_running_bare() {
    let workspace = TempDir::new().expect("temp dir");
    let policy = Policy::workspace_write(workspace.path());
    // A missing runner makes `enforcement()` `None`, so `wrap` has to refuse.
    let broken = Landlock::with_runner("/no-such-file");
    let err = broken
        .wrap(vec!["/bin/echo".into(), "hello".into()], &policy)
        .expect_err("no confinement means nothing runs");
    // Returning bare argv would drop the boundary just when the user believes in it.
    assert!(err.to_string().contains("không giam được"), "{err}");
}

/// Opt-in network confinement really blocks a TCP connect, to a port this test opened itself.
#[test]
fn deny_network_really_blocks_a_tcp_connection() {
    if !can_confine() {
        return;
    }
    if !provider().network_confinable() {
        eprintln!("skipped: this kernel is below Landlock ABI 4, which has no network rules");
        return;
    }

    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().canonicalize().expect("canonicalises");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("opens a port");
    let port = listener.local_addr().expect("has an address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            drop(stream);
        }
    });

    // dash has no `/dev/tcp`, so the probe uses python3 when present and skips otherwise.
    let probe = format!(
        "command -v python3 >/dev/null || exit 111; \
         python3 -c 'import socket,sys; s=socket.socket(); s.settimeout(2); \
         sys.exit(0 if s.connect_ex((\"127.0.0.1\",{port}))==0 else 1)'"
    );

    let open = Policy::new(Mode::WorkspaceWrite, &root);
    if !runs(&open, &probe) {
        eprintln!("skipped: no connection even unconfined, so the deny cannot be tested");
        return;
    }

    let denied = Policy::new(Mode::WorkspaceWrite, &root).deny_network();
    assert!(
        !runs(&denied, &probe),
        "`deny_network` is set and TCP still connected: the Landlock ruleset denies nothing"
    );
}

/// Denying the network must not weaken file confinement: both share one ruleset.
#[test]
fn denying_the_network_leaves_file_confinement_intact() {
    if !can_confine() || !provider().network_confinable() {
        return;
    }
    let workspace = TempDir::new().expect("temp dir");
    let root = workspace.path().canonicalize().expect("canonicalises");

    // `/tmp` and `/var/tmp` are deliberately writable, so the outside path is under home.
    let home = std::env::var("HOME").expect("HOME is set");
    let blocked = Path::new(&home).join(format!("pai-net-must-not-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&blocked);

    let policy = Policy::new(Mode::WorkspaceWrite, &root).deny_network();
    assert!(
        runs(&policy, &format!("echo x > {}/trong.txt", root.display())),
        "writes inside the workspace must still work when the network is denied"
    );
    assert!(
        !runs(&policy, &format!("echo x > {}", blocked.display())),
        "denying the network must not loosen the file confinement"
    );
    assert!(!blocked.exists(), "no file outside the workspace may be created");
}
