//! Is the confinement real.
//!
//! These tests deliberately **run real commands** rather than compare profile strings. A
//! syntactically correct but semantically wrong SBPL profile passes every string comparison
//! and confines nothing — SBPL takes the *last* matching rule, so swapping two clauses makes
//! the profile harmless while it still looks identical.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Stdio};

use pai_sandbox::seam::SandboxProvider;
use pai_sandbox::{Mode, Policy};
use tempfile::TempDir;

/// Run a shell command through the sandbox. Returns `true` when it succeeds.
fn runs(policy: &Policy, command: &str) -> bool {
    let Some(seatbelt) = pai_sandbox::seatbelt::Seatbelt::detect() else {
        // If the probe fails, skip rather than go red: a CI machine inside App Sandbox
        // cannot run `sandbox-exec`, and a test that goes red because of the environment is
        // a test nobody trusts any more.
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

    // Do **not** use a `TempDir` as the "outside" location: `writable_roots` deliberately
    // allows writing the temp directory, so a temp file sits inside the allowed area and
    // the test would wrongly conclude the sandbox does not confine. The user's home
    // directory really is outside.
    let home = std::env::var("HOME").expect("HOME is set");
    let target = Path::new(&home).join(format!(
        ".pai-sandbox-must-not-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&target);

    assert!(
        !runs(&policy, &format!("echo x > {}", target.display())),
        "writing outside the workspace has to fail"
    );
    // And it failed because it was blocked, not because the command was wrong: the file was
    // never created.
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
    // This mode is the *absence* of a sandbox. Wrapping it would build an empty boundary
    // that then has to be maintained across every release.
    assert_eq!(wrapped, argv);
    assert_eq!(policy.mode, Mode::DangerFullAccess);
}

#[test]
fn a_provider_that_does_not_confine_never_reports_that_it_does() {
    // `Enforcement` is reported truth, not a promise: a lying sandbox is more dangerous than
    // no sandbox, because the user clicks "allow" on the strength of it.
    let unconfined = pai_sandbox::Unconfined::new("máy này không có gì để giam");
    assert!(!unconfined.enforcement().confines());
    assert!(unconfined.enforcement().reason().is_some());
}

/// Opt-in network confinement really cuts the process off.
///
/// The point of testing this by *running* something is the same as everywhere else in this
/// file: `(deny network*)` in the right place blocks, and the same clause one line earlier
/// is overridden by `(allow default)` and blocks nothing. Both profiles read fine.
///
/// The target is a TCP connect to a port on this machine, not a name on the internet: a
/// machine with no network at all would make an internet probe pass for the wrong reason,
/// and that is how a security test quietly stops testing anything.
#[test]
fn deny_network_really_blocks_a_connection() {
    let dir = TempDir::new().expect("temp dir");

    // Open a listening port so that "it connected" is a checkable fact rather than an
    // assumption about the environment.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("opens a port");
    let port = listener.local_addr().expect("has an address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            drop(stream);
        }
    });

    let probe = format!(
        "/usr/bin/nc -z -w 2 127.0.0.1 {port} || exit 1"
    );

    // Unconfined: it connects. If this half fails the other half proves nothing — it would
    // "block" merely because `nc` could not run.
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

/// The default does **not** change: without an explicit opt-in the profile says nothing
/// about the network.
///
/// This guards a product decision, not an implementation detail. Denying the network by
/// default breaks `cargo` and `npm`, so one well-meaning "let's turn it on to be safe" shows
/// up as an agent that cannot fetch a dependency, and nobody traces it back.
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
