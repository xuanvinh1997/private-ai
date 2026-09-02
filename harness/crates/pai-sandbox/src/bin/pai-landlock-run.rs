//! Bind yourself, then `exec`.
//!
//! Landlock confines **the process that calls it**, not some other process. There is no
//! "run this command in a box" API like macOS's `sandbox-exec` — so the boundary has to be
//! built inside the very process that is about to become that command, in the moment after
//! `fork` and before `exec`. This binary *is* that moment.
//!
//! It takes the policy on the command line, applies it to itself, then replaces itself with
//! the real command. After `exec` it no longer exists; what remains is the user's command,
//! confined.
//!
//! Exit codes for failures **before** `exec` deliberately sit outside the range commands
//! normally use, so a failure here is not misread as a failure of the command: `2` is bad
//! arguments, `3` is no confinement.

fn main() {
    #[cfg(target_os = "linux")]
    linux::main();

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("pai-landlock-run chỉ chạy trên Linux");
        std::process::exit(2);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use landlock::{
        ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus, path_beneath_rules,
    };

    /// The ABI that introduced network rules. Below this the kernel cannot confine TCP at
    /// all, and the caller has to be told rather than left believing otherwise.
    const NET_ABI: ABI = ABI::V4;

    /// The highest ABI this crate knows. `BestEffort` steps down to match the running
    /// kernel, and `RulesetStatus` tells us how far it stepped — that is where
    /// `Enforcement::Partial` comes from, rather than from a guess.
    const DESIRED_ABI: ABI = ABI::V5;

    pub fn main() {
        let mut writable: Vec<String> = Vec::new();
        let mut argv: Vec<String> = Vec::new();
        let mut deny_network = false;
        let mut after_dashes = false;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if after_dashes {
                argv.push(arg);
            } else if arg == "--" {
                after_dashes = true;
            } else if arg == "--deny-network" {
                deny_network = true;
            } else if arg == "--allow-write" {
                match args.next() {
                    Some(path) => writable.push(path),
                    None => fail(2, "--allow-write thiếu đường dẫn"),
                }
            } else {
                fail(2, &format!("tham số lạ: {arg}"));
            }
        }
        if argv.is_empty() {
            fail(2, "không có lệnh nào để chạy");
        }

        let status = match build_ruleset(&writable, deny_network) {
            Ok(status) => status,
            Err(err) => fail(3, &format!("không dựng được vòng giam: {err}")),
        };
        if matches!(status, RulesetStatus::NotEnforced) {
            // No confinement means **nothing runs**. Carrying on lets the caller believe
            // in a boundary that does not exist, which is more dangerous than having no
            // sandbox at all.
            fail(3, "kernel không thi hành được Landlock");
        }

        let err = Command::new(&argv[0]).args(&argv[1..]).exec();
        // `exec` only returns when it fails.
        fail(3, &format!("không chạy được {}: {err}", argv[0]));
    }

    /// Read everywhere, write only inside the given roots.
    ///
    /// Leaving reads open is deliberate and identical to the macOS version: a coding agent
    /// has to read the repo, the toolchain, the dependency cache and the git config;
    /// punching enough holes to make it work leaves the read boundary meaningless. What
    /// blocks reading secrets is `pai-fs`'s protected-path list, not this file.
    fn build_ruleset(
        writable: &[String],
        deny_network: bool,
    ) -> Result<RulesetStatus, Box<dyn std::error::Error>> {
        let mut base = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(DESIRED_ABI))?;

        // Network. Landlock denies every *handled* access that no rule allows, so handling
        // TCP and then adding no `NetPort` rule is a total block on bind and connect.
        //
        // Two things this does **not** do, both of which have to reach the user rather than
        // stay in a comment. It is **TCP only** — Landlock has no UDP verb, so DNS and any
        // UDP transport still leave the box. And a socket connected *before* the ruleset is
        // applied stays usable; the boundary binds new connections, not existing ones.
        if deny_network {
            base = base.handle_access(AccessNet::from_all(NET_ABI))?;
        }

        let mut ruleset = base
            .create()?
            // `/` is open for reading. Without this rule the process cannot even read the
            // binary it is about to `exec`.
            .add_rules(path_beneath_rules(
                ["/"],
                AccessFs::from_read(DESIRED_ABI),
            ))?
            // The mandatory hole: nearly every command opens `/dev/null` to discard
            // output, so without it `read-only` cannot run a single command.
            //
            // This is where Linux is **wider** than macOS, and that was measured, not
            // guessed: a `path_beneath` rule pointing straight at `/dev/null` cannot grant
            // write access to it under any combination of permissions — Landlock does not
            // govern device nodes per file. A narrow permission set on the `/dev` directory
            // is not enough either. The macOS SBPL profile opens exactly one file; here the
            // whole of `/dev` has to be opened as a writable root.
            //
            // What that actually permits is far narrower than it looks: on a real machine
            // `/dev` belongs to root, so an agent running as the user can only write device
            // nodes that were already world-writable — `/dev/null`, `/dev/zero`,
            // `/dev/tty`.
            .add_rules(path_beneath_rules(
                ["/dev"],
                AccessFs::from_all(DESIRED_ABI),
            ))?;

        for path in writable {
            // A root that does not exist is skipped rather than fatal: `writable_roots`
            // already filtered, but a directory can vanish between filtering and running.
            if std::path::Path::new(path).exists() {
                ruleset = ruleset.add_rules(path_beneath_rules(
                    [path],
                    AccessFs::from_all(DESIRED_ABI),
                ))?;
            }
        }

        Ok(ruleset.restrict_self()?.ruleset)
    }

    fn fail(code: i32, message: &str) -> ! {
        eprintln!("pai-landlock-run: {message}");
        std::process::exit(code);
    }
}
