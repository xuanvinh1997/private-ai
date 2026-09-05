//! One repository, and the only place in this crate that starts a process.
//!
//! Three rules hold everything else up:
//! 1. Arguments are an `argv` array. Nothing is ever concatenated into a command line and
//!    nothing goes near a shell, so a branch name the model invented cannot become a
//!    command. `--` separates options from operands wherever git accepts it.
//! 2. Output is bounded before it is a `String`. A `git diff` across a vendored tree can be
//!    hundreds of megabytes, and a process that buffers it has already lost.
//! 3. The cancel token kills the whole process group, not just `git`. Git spawns helpers,
//!    and a helper that outlives its parent keeps writing to a pipe nobody reads.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::error::GitError;

/// Most git output we keep, in bytes (8 MiB). Well past any result the model can use — the
/// token budget cuts at roughly 24 KiB — but high enough that we never truncate a diff the
/// caller then wants whole from the spill store.
pub const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;

/// stderr only ever carries a message for a human, so 64 KiB is generous.
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Lines of stderr quoted back when git exits non-zero. Enough to tell a bad revision from a
/// missing file; not so much that a wall of hints buries the first line, which is the real one.
const STDERR_LINES_QUOTED: usize = 4;

/// Grace between SIGTERM and SIGKILL for a cancelled command; git needs only enough time to
/// unlink its temporary files.
const KILL_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

/// One command's output, already decoded and already bounded.
#[derive(Debug)]
pub struct GitOutput {
    pub stdout: String,
    /// Git wrote more than [`MAX_STDOUT_BYTES`]; whatever is in `stdout` stops mid-stream.
    pub overflowed: bool,
}

/// A git repository at a fixed path.
///
/// The path is set once, by the plugin, from the open project — it is never a tool
/// parameter. That is the same shape `RagPlugin` uses for its library root, and it removes
/// a whole class of question: there is no repository argument for the model to point
/// somewhere else, so the only paths that need checking are the pathspecs inside a call,
/// and [`Repo::relative`] checks those against this root.
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    pub fn new(root: PathBuf) -> Repo {
        Repo { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run one git command inside the repository.
    ///
    /// `args` is everything after the global options; it is passed through untouched, so
    /// callers are responsible for having validated any string that came from the model
    /// with [`check_rev`] or [`Repo::relative`] first.
    pub async fn run(
        &self,
        args: &[String],
        cancel: &CancellationToken,
    ) -> Result<GitOutput, GitError> {
        let mut cmd = Command::new("git");
        cmd
            // A pager attached to a piped stdout would be harmless, but a pager attached to
            // anything is one more process in the group we would have to kill.
            .arg("--no-pager")
            // Print non-ASCII filenames as themselves. The default octal-escapes them, and a
            // model reading `\303\251` cannot pass that path back to us as an argument.
            .args(["-c", "core.quotepath=false"])
            // ANSI escapes are not information here; they are tokens and confusion.
            .args(["-c", "color.ui=false"]);
        cmd.args(args);
        cmd.current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            // Stable English, so our error matching and our parsers do not depend on which
            // locale the user happens to run. Every date format we ask for is numeric, so
            // this costs no readability.
            .env("LC_ALL", "C")
            // `git status` refreshes the index and takes a lock to do it. This is a read-only
            // tool running beside a human's editor; taking that lock is how we make their
            // `git commit` fail with "index.lock exists".
            .env("GIT_OPTIONAL_LOCKS", "0")
            // Same three as `pai-project/src/clone.rs`: a child with no terminal that is
            // asked for a password waits forever. None of these commands should reach the
            // network, but `git log` on a repo with a promisor remote will try.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .env("SSH_ASKPASS", "");

        // Signalling the group requires being the group leader; without this, killing on
        // cancel reaches `git` and leaves its children behind.
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn().map_err(|err| {
            if err.kind() != std::io::ErrorKind::NotFound {
                return GitError::Io(err.to_string());
            }
            // ENOENT from `spawn` is ambiguous: it is either the binary or the working
            // directory. Blaming PATH when the project folder was renamed underneath us sends
            // the user hunting for the wrong thing entirely, so ask the disk which it was.
            if self.root.is_dir() {
                GitError::Missing(err.to_string())
            } else {
                GitError::RootGone(self.root.clone())
            }
        })?;

        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(GitError::Io("không lấy được đầu ra của `git`".into()));
        };

        // Both pipes are drained at once. Reading one to the end first deadlocks the moment
        // git fills the other one's buffer.
        let pumped = tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            pair = async {
                tokio::join!(
                    read_capped(stdout, MAX_STDOUT_BYTES),
                    read_capped(stderr, MAX_STDERR_BYTES),
                )
            } => Some(pair),
        };
        let Some(((out, overflowed), (err, _))) = pumped else {
            kill_group(&mut child).await;
            return Err(GitError::Cancelled);
        };

        let waited = tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            status = child.wait() => Some(status),
        };
        let Some(status) = waited else {
            kill_group(&mut child).await;
            return Err(GitError::Cancelled);
        };
        let status = status.map_err(|err| GitError::Io(err.to_string()))?;

        // Lossy on purpose: a filename can be any byte sequence, and a mojibake path in a
        // listing is a far better outcome than refusing to report the listing at all.
        let stdout = String::from_utf8_lossy(&out).into_owned();
        if status.success() {
            return Ok(GitOutput { stdout, overflowed });
        }

        let stderr = String::from_utf8_lossy(&err);
        // Answer the most common failure with the sentence that actually helps, rather than
        // with git's own wording buried inside an exit code.
        if stderr.contains("not a git repository") {
            return Err(GitError::NotARepo(self.root.clone()));
        }
        let detail = summarize_stderr(&stderr);
        Err(GitError::Command {
            command: args.join(" "),
            code: status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "bị tín hiệu dừng".to_string()),
            detail,
        })
    }

    /// Turn a caller's path into one git will read as a path inside this repository.
    ///
    /// Absolute paths are accepted only if they sit under the root; relative ones are
    /// resolved lexically and refused the moment `..` walks out. Lexical, not
    /// `canonicalize`: a pathspec is allowed to name a file that no longer exists in the
    /// working tree — that is the normal case for `git log -- <deleted file>`.
    pub fn relative(&self, raw: &str) -> Result<String, GitError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(GitError::Empty("đường dẫn"));
        }
        if raw.starts_with('-') {
            return Err(GitError::LeadingDash(raw.to_string()));
        }
        if raw.starts_with(':') {
            return Err(GitError::MagicPathspec(raw.to_string()));
        }
        if raw.chars().any(|ch| ch.is_control()) {
            return Err(GitError::ControlChar(raw.to_string()));
        }

        let given = Path::new(raw);
        let inside = if given.is_absolute() {
            // Compare lexically on both sides: the root came from the project and the path
            // came from the model, and neither is guaranteed to exist yet.
            let root = lexical(&self.root);
            lexical(given)
                .strip_prefix(&root)
                .map(Path::to_path_buf)
                .map_err(|_| GitError::OutsideRepo(raw.to_string(), self.root.clone()))?
        } else {
            lexical(given)
        };

        // A leading `..` survives `lexical` exactly when the path escaped; see the note there.
        if inside.components().next() == Some(Component::ParentDir) {
            return Err(GitError::OutsideRepo(raw.to_string(), self.root.clone()));
        }
        let cleaned = inside.to_string_lossy().replace('\\', "/");
        if cleaned.is_empty() {
            // `.` normalises to nothing, and an empty pathspec matches everything, which is
            // not what someone writing `.` was refused for.
            return Ok(".".to_string());
        }
        Ok(cleaned)
    }

    /// [`Repo::relative`] over a list, so a tool can validate every pathspec in one line.
    pub fn relatives(&self, raw: &[String]) -> Result<Vec<String>, GitError> {
        raw.iter().map(|item| self.relative(item)).collect()
    }
}

/// A revision from the model: branch, tag, sha, `HEAD~2`, `main..dev`, whatever git accepts.
///
/// We do not try to decide what is a valid revision — git owns that grammar and will say so
/// itself. We only refuse the two shapes that stop being an operand: a leading `-`, which
/// git reads as an option however many `--` we put in front of it, and control characters,
/// which are never part of a name anyone typed.
pub fn check_rev(raw: &str) -> Result<String, GitError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(GitError::Empty("phiên bản"));
    }
    if raw.starts_with('-') {
        return Err(GitError::LeadingDash(raw.to_string()));
    }
    if raw.chars().any(|ch| ch.is_control()) {
        return Err(GitError::ControlChar(raw.to_string()));
    }
    Ok(raw.to_string())
}

/// A free-text filter such as `--author=` or `--grep=`. Same reasoning as [`check_rev`],
/// except these are always glued to their option with `=`, so a leading `-` is harmless and
/// allowed — an author really can be called `-h`.
pub fn check_text(raw: &str, what: &'static str) -> Result<String, GitError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(GitError::Empty(what));
    }
    if raw.chars().any(|ch| ch.is_control()) {
        return Err(GitError::ControlChar(raw.to_string()));
    }
    Ok(raw.to_string())
}

/// Resolve `.` and `..` without touching the disk. Mirrors `pai_fs::path::lexical`; copied
/// rather than imported, since a whole dependency on `pai-fs` for six lines buys nothing.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                // A leading `..` has nothing to pop; keeping it is what lets the caller see
                // that the path escaped.
                if !out.pop() {
                    out.push(Component::ParentDir);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Read to EOF, keeping at most `cap` bytes.
///
/// Reading continues past the cap with the extra bytes thrown away. Stopping early would be
/// cheaper and would also deadlock: git blocks writing into a pipe nobody drains, never
/// exits, and the other pipe never reaches EOF.
async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: usize) -> (Vec<u8>, bool) {
    let mut kept: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut overflowed = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => {
                let room = cap.saturating_sub(kept.len());
                if room >= read {
                    kept.extend_from_slice(&chunk[..read]);
                } else {
                    kept.extend_from_slice(&chunk[..room]);
                    overflowed = true;
                }
            }
            // A broken pipe here means the child died; `wait` below reports why.
            Err(_) => break,
        }
    }
    (kept, overflowed)
}

/// The tail of git's complaint, as one line ready to append to an error sentence.
fn summarize_stderr(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(STDERR_LINES_QUOTED);
    format!(": {}", lines[start..].join(" / "))
}

/// SIGTERM then SIGKILL the whole group, exactly as `pai-project/src/clone.rs` does; on a
/// platform without process groups the direct kill is all there is.
async fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let pid = pid as i32;
        // Safety: `kill` with a negative pid signals a process group and cannot corrupt
        // memory; the worst case is signalling a group that already exited, which is ESRCH.
        unsafe { libc::kill(-pid, libc::SIGTERM) };
        tokio::time::sleep(KILL_GRACE).await;
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    let _ = child.kill().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        Repo::new(PathBuf::from("/kho"))
    }

    #[test]
    fn relative_keeps_a_path_inside_the_repo() {
        assert_eq!(repo().relative("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(repo().relative("./src/../src/a.rs").unwrap(), "src/a.rs");
        assert_eq!(repo().relative("/kho/src/a.rs").unwrap(), "src/a.rs");
    }

    #[test]
    fn relative_refuses_an_escape() {
        assert!(matches!(
            repo().relative("../ngoai/a.rs"),
            Err(GitError::OutsideRepo(..))
        ));
        assert!(matches!(
            repo().relative("src/../../a.rs"),
            Err(GitError::OutsideRepo(..))
        ));
        assert!(matches!(
            repo().relative("/etc/passwd"),
            Err(GitError::OutsideRepo(..))
        ));
    }

    #[test]
    fn relative_refuses_an_option_or_a_magic_pathspec() {
        assert!(matches!(
            repo().relative("--output=/tmp/x"),
            Err(GitError::LeadingDash(_))
        ));
        assert!(matches!(
            repo().relative(":(exclude)src"),
            Err(GitError::MagicPathspec(_))
        ));
        assert!(matches!(repo().relative("  "), Err(GitError::Empty(_))));
    }

    #[test]
    fn check_rev_allows_real_revisions_and_refuses_options() {
        assert_eq!(check_rev(" HEAD~2 ").unwrap(), "HEAD~2");
        assert_eq!(check_rev("main..dev").unwrap(), "main..dev");
        assert!(matches!(
            check_rev("--upload-pack=sh"),
            Err(GitError::LeadingDash(_))
        ));
        assert!(matches!(
            check_rev("main\nrm -rf"),
            Err(GitError::ControlChar(_))
        ));
    }

    #[tokio::test]
    async fn read_capped_marks_overflow_and_still_drains() {
        let data = [b'x'; 100];
        let (kept, overflowed) = read_capped(&data[..], 10).await;
        assert_eq!(kept.len(), 10);
        assert!(overflowed);

        let (kept, overflowed) = read_capped(&data[..], 1000).await;
        assert_eq!(kept.len(), 100);
        assert!(!overflowed);
    }

    #[tokio::test]
    async fn a_missing_root_is_not_reported_as_a_missing_git() {
        // Both failures are ENOENT from `spawn`; only one of them is the user's fault, and
        // telling them to install `git` when the folder moved wastes the whole diagnosis.
        let repo = Repo::new(PathBuf::from("/khong-ton-tai-o-dau-ca"));
        let err = repo
            .run(&["status".to_string()], &CancellationToken::new())
            .await
            .expect_err("thư mục không tồn tại thì không chạy được");
        assert!(matches!(err, GitError::RootGone(_)), "{err:?}");
    }

    #[test]
    fn summarize_stderr_keeps_the_last_lines() {
        assert_eq!(summarize_stderr("   \n\n"), "");
        let many = (1..=10).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
        assert_eq!(summarize_stderr(&many), ": 7 / 8 / 9 / 10");
    }
}
