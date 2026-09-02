//! The command execution seam, and its local implementation.
//!
//! The one thing worth remembering in this file: **what we run is a process tree, not a
//! process.** `sh -c "npm test"` spawns `npm`, which spawns `node`. Killing the shell
//! leaves both of those behind, holding ports, holding file locks, still writing into the
//! same directory — so after one cancelled turn, the next one runs on a contaminated
//! machine.
//!
//! So the child is placed in its **own process group**, and every signal goes to the whole
//! group. This is the easiest thing in the crate to get wrong, and also the kind of wrong
//! that never announces itself: everything still looks like it works.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pai_core::{Context, ServiceKey};
use pai_sandbox::{Policy, Sandbox};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct Execution {
    pub output: String,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    /// Cut short by a timeout or a cancellation. Partial output is still useful, but the
    /// fact that it is partial has to be said.
    pub interrupted: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("không chạy được lệnh: {0}")]
    Spawn(String),
}

pub struct Request {
    pub command: String,
    pub cwd: std::path::PathBuf,
    pub timeout: Option<Duration>,
    pub cancel: CancellationToken,
}

#[async_trait]
pub trait ShellExecutor: Send + Sync + 'static {
    async fn run(&self, req: Request) -> Result<Execution, ShellError>;
}

pub enum Shell {}
impl ServiceKey for Shell {
    type Api = dyn ShellExecutor;
    const NAME: &'static str = "shell";
}

/// Signal the whole process group.
#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    // A negative pid means "the whole group". This is the entire reason for setting the
    // process group above.
    unsafe { libc::kill(-(pid as i32), signal) };
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {
    // Windows has no process group in this sense; the equivalent is a Job Object, and
    // that belongs to `pai-sandbox`. Until then, only the direct child is killed.
}

/// This machine's own disk.
///
/// It holds a `Context` so it can ask for the sandbox seam **at spawn time**, not at
/// construction time: disposing the sandbox plugin must make every subsequent command run
/// unconfined, rather than going through a leftover copy of the old provider.
pub struct LocalShell {
    ctx: Context,
    policy: Policy,
}

impl LocalShell {
    pub fn new(ctx: Context, policy: Policy) -> LocalShell {
        LocalShell { ctx, policy }
    }

    /// argv after wrapping, if anything wrapped it.
    ///
    /// With no provider, the original argv runs — that is the correct behaviour when
    /// nobody has mounted a sandbox. But a provider that **is** present and **fails** to
    /// wrap means nothing runs: one silent fallthrough is one user trusting a confinement
    /// that does not exist.
    fn argv(&self, command: &str) -> Result<Vec<String>, ShellError> {
        let argv = vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()];
        match self.ctx.get::<Sandbox>() {
            Some(sandbox) => sandbox
                .wrap(argv, &self.policy)
                .map_err(|err| ShellError::Spawn(err.to_string())),
            None => Ok(argv),
        }
    }
}

#[async_trait]
impl ShellExecutor for LocalShell {
    async fn run(&self, req: Request) -> Result<Execution, ShellError> {
        let argv = self.argv(&req.command)?;
        let (program, args) = argv
            .split_first()
            .ok_or(ShellError::Spawn("argv rỗng".into()))?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&req.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .map_err(|err| ShellError::Spawn(err.to_string()))?;
        let pid = child.id();

        // stdout and stderr are interleaved in arrival order, not split into two blocks:
        // an error line printed midway only means something once you know which lines it
        // sits between.
        let collected = Arc::new(Mutex::new(String::new()));
        let mut pumps = Vec::new();
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let sink = collected.clone();
            pumps.push(tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut buffer = sink.lock();
                    buffer.push_str(&line);
                    buffer.push('\n');
                }
            }));
        }

        let waited = async { child.wait().await };
        let interrupted = tokio::select! {
            status = waited => {
                let status = status.map_err(|err| ShellError::Spawn(err.to_string()))?;
                for pump in pumps.drain(..) {
                    let _ = pump.await;
                }
                let output = collected.lock().clone();
                return Ok(Execution {
                    output,
                    exit_code: status.code(),
                    signal: exit_signal(&status),
                    interrupted: None,
                });
            }
            _ = sleep_opt(req.timeout) => Some(format!(
                "lệnh bị dừng sau {} giây",
                req.timeout.map(|t| t.as_secs()).unwrap_or_default()
            )),
            _ = req.cancel.cancelled() => Some("lượt đã bị huỷ".to_string()),
        };

        // Kill the whole group, not just the shell.
        if let Some(pid) = pid {
            signal_group(pid, libc_sigterm());
            // Give the process tree a moment to clean up after itself before forcing.
            tokio::time::sleep(Duration::from_millis(200)).await;
            signal_group(pid, libc_sigkill());
        }
        let _ = child.kill().await;
        for pump in pumps.drain(..) {
            let _ = pump.await;
        }

        Ok(Execution {
            output: collected.lock().clone(),
            exit_code: None,
            signal: None,
            interrupted,
        })
    }
}

/// `None` means no deadline: wait forever, rather than waiting zero seconds.
async fn sleep_opt(duration: Option<Duration>) {
    match duration {
        Some(duration) => tokio::time::sleep(duration).await,
        None => std::future::pending().await,
    }
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|s| format!("SIG{s}"))
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<String> {
    None
}

#[cfg(unix)]
fn libc_sigterm() -> i32 {
    libc::SIGTERM
}
#[cfg(unix)]
fn libc_sigkill() -> i32 {
    libc::SIGKILL
}
#[cfg(not(unix))]
fn libc_sigterm() -> i32 {
    0
}
#[cfg(not(unix))]
fn libc_sigkill() -> i32 {
    0
}
