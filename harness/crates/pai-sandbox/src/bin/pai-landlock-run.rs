//! Bind yourself, then `exec`: Landlock confines only the calling process, so this binary
//! takes the policy on its command line, applies it to itself, and becomes the real command.
//! Pre-`exec` failures use codes outside the usual range: `2` bad arguments, `3` no confinement.

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

    /// The ABI that introduced network rules; below it the kernel cannot confine TCP at all.
    const NET_ABI: ABI = ABI::V4;

    /// Highest ABI this crate knows; `BestEffort` steps down and `RulesetStatus` reports how far.
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
            // No confinement means nothing runs, or the caller believes in a boundary that is not there.
            fail(3, "kernel không thi hành được Landlock");
        }

        let err = Command::new(&argv[0]).args(&argv[1..]).exec();
        // `exec` only returns when it fails.
        fail(3, &format!("không chạy được {}: {err}", argv[0]));
    }

    /// Read everywhere, write only inside the given roots; secrets are blocked by `pai-fs` instead.
    fn build_ruleset(
        writable: &[String],
        deny_network: bool,
    ) -> Result<RulesetStatus, Box<dyn std::error::Error>> {
        let mut base = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(DESIRED_ABI))?;

        // Handling TCP with no `NetPort` rule blocks bind and connect; UDP and open sockets remain.
        if deny_network {
            base = base.handle_access(AccessNet::from_all(NET_ABI))?;
        }

        let mut ruleset = base
            .create()?
            // `/` is readable, or the process cannot even read the binary it is about to `exec`.
            .add_rules(path_beneath_rules(["/"], AccessFs::from_read(DESIRED_ABI)))?
            // The `/dev/null` hole: Landlock cannot grant it per file, so all of `/dev` opens.
            .add_rules(path_beneath_rules(
                ["/dev"],
                AccessFs::from_all(DESIRED_ABI),
            ))?;

        for path in writable {
            // A missing root is skipped, not fatal: it can vanish between filtering and running.
            if std::path::Path::new(path).exists() {
                ruleset = ruleset
                    .add_rules(path_beneath_rules([path], AccessFs::from_all(DESIRED_ABI)))?;
            }
        }

        Ok(ruleset.restrict_self()?.ruleset)
    }

    fn fail(code: i32, message: &str) -> ! {
        eprintln!("pai-landlock-run: {message}");
        std::process::exit(code);
    }
}
