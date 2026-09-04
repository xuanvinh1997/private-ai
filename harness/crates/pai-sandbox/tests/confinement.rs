//! Is the confinement real. These tests run real commands rather than compare profile text,
//! because SBPL takes the last matching rule: two swapped clauses look identical and confine nothing.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Stdio};

use pai_sandbox::seam::SandboxProvider;
use pai_sandbox::{Mode, Policy};
use tempfile::TempDir;

/// Run a shell command through the sandbox. Returns `true` when it succeeds.
fn runs(policy: &Policy, command: &str) -> bool {
    let Some(seatbelt) = pai_sandbox::seatbelt::Seatbelt::detect() else {
        // Skip rather than go red: a CI machine inside App Sandbox cannot run `sandbox-exec`.
        eprintln!("skipped: this machine cannot run sandbox-exec");
        return true;
    };
    let argv = seatbelt
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

fn bench() -> (TempDir, TempDir) {
    (
        TempDir::new().expect("workspace"),
        TempDir::new().expect("outside the workspace"),
    )
}

#[test]
fn workspace_write_allows_writes_inside_the_workspace() {
    let (workspace, _) = bench();
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
    let (workspace, _) = bench();
    let root = workspace.path().canonicalize().expect("canonicalises");
    let policy = Policy::workspace_write(&root);

    // Not a `TempDir` as the outside location: the temp directory is deliberately writable.
    let home = std::env::var("HOME").expect("HOME is set");
    let target = Path::new(&home).join(format!(".pai-sandbox-must-not-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&target);

    assert!(
        !runs(&policy, &format!("echo x > {}", target.display())),
        "writing outside the workspace has to fail"
    );
    // It failed because it was blocked, not because the command was wrong: no file appeared.
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
    let (workspace, _) = bench();
    let root = workspace.path().canonicalize().expect("canonicalises");
    let policy = Policy::read_only(&root);

    assert!(!runs(
        &policy,
        &format!("echo x > {}/a.txt", root.display())
    ));
    // But reading still has to work: a read-only agent that cannot read is useless.
    assert!(runs(&policy, "/bin/ls / > /dev/null"));
}

#[test]
fn danger_full_access_wraps_nothing() {
    let (workspace, _) = bench();
    let policy = Policy::danger_full_access(workspace.path());
    let argv = vec!["/bin/echo".to_string(), "hello".to_string()];

    let seatbelt = pai_sandbox::seatbelt::Seatbelt::with_runner("/usr/bin/sandbox-exec");
    let wrapped = seatbelt.wrap(argv.clone(), &policy).expect("wraps");
    // This mode is the absence of a sandbox; wrapping it would build an empty boundary.
    assert_eq!(wrapped, argv);
    assert_eq!(policy.mode, Mode::DangerFullAccess);
}

#[test]
fn a_provider_that_does_not_confine_never_reports_that_it_does() {
    // `Enforcement` is reported truth: a lying sandbox is worse than no sandbox at all.
    let unconfined = pai_sandbox::Unconfined::new("máy này không có gì để giam");
    assert!(!unconfined.enforcement().confines());
    assert!(unconfined.enforcement().reason().is_some());
}

/// Opt-in network confinement really cuts the process off, tested against a locally opened port.
#[test]
fn deny_network_really_blocks_a_connection() {
    let dir = TempDir::new().expect("temp dir");

    // Open a listening port, so "it connected" is a checkable fact about this machine.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("opens a port");
    let port = listener.local_addr().expect("has an address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            drop(stream);
        }
    });

    let probe = format!("/usr/bin/nc -z -w 2 127.0.0.1 {port} || exit 1");

    // Unconfined it connects; without this half a block could just mean `nc` never ran.
    let open = Policy::new(Mode::WorkspaceWrite, dir.path());
    if !runs(&open, &probe) {
        eprintln!("skipped: cannot reach a port this test opened, so the deny cannot be tested");
        return;
    }

    let denied = Policy::new(Mode::WorkspaceWrite, dir.path()).deny_network();
    assert!(
        !runs(&denied, &probe),
        "`deny_network` is set and the connection still succeeded: the SBPL profile denies nothing"
    );
}

/// Without an opt-in the profile says nothing about the network; denying it by default breaks `cargo`.
#[test]
fn the_default_still_leaves_the_network_alone() {
    let dir = TempDir::new().expect("temp dir");
    let profile = pai_sandbox::seatbelt::profile(&Policy::new(Mode::WorkspaceWrite, dir.path()));
    assert!(
        !profile.contains("network"),
        "the default profile must not mention the network, but it is:\n{profile}"
    );

    let denied = pai_sandbox::seatbelt::profile(
        &Policy::new(Mode::WorkspaceWrite, dir.path()).deny_network(),
    );
    let allow_at = denied.find("(allow default)").expect("has allow default");
    let deny_at = denied.find("(deny network*)").expect("has deny network");
    assert!(
        deny_at > allow_at,
        "SBPL takes the **last** matching rule: `(deny network*)` placed before \
         `(allow default)` is swallowed and the profile denies nothing.\n{denied}"
    );
}
