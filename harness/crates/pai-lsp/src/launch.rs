//! Opens a pipe to a language server, and finds out whether one exists on this machine.
//! Split from [`crate::client`] for the reason `pai-mcp` splits its dialer: the tests need
//! a way to open a pipe that spawns no child process.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::Child;

/// A two-way pipe to a running server.
pub struct Channel {
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    /// Only present for child-process servers; kept so we can kill one that ignores `exit` instead of leaving it orphaned.
    pub(crate) child: Option<Child>,
}

impl Channel {
    pub fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Channel {
        Channel {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: None,
        }
    }
}

/// How to open a pipe. One call = one new server.
#[async_trait]
pub trait Launch: Send + Sync + 'static {
    async fn launch(&self) -> anyhow::Result<Channel>;
    /// The name to use in error messages - what the user recognizes, not what we typed.
    fn label(&self) -> String;
}

/// A child process speaking LSP over stdin/stdout.
pub struct ChildLaunch {
    label: String,
    command: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

impl ChildLaunch {
    pub fn new(
        label: impl Into<String>,
        command: impl Into<PathBuf>,
        args: Vec<String>,
        cwd: impl Into<PathBuf>,
    ) -> ChildLaunch {
        ChildLaunch {
            label: label.into(),
            command: command.into(),
            args,
            cwd: cwd.into(),
        }
    }
}

#[async_trait]
impl Launch for ChildLaunch {
    async fn launch(&self) -> anyhow::Result<Channel> {
        let mut command = tokio::process::Command::new(&self.command);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is inherited, as in `pai-mcp`: a server that fails to start almost always says why there.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("không lấy được stdin của `{}`", self.label))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("không lấy được stdout của `{}`", self.label))?;

        Ok(Channel {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            child: Some(child),
        })
    }

    fn label(&self) -> String {
        self.label.clone()
    }
}

/// Does this command exist on the machine, and where? Probed once at plugin time, never per call, because a tool that always fails teaches the model to ignore the tool list.
pub fn locate(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    // A directory separator means the user named it outright; `PATH` is irrelevant.
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        let path = PathBuf::from(command);
        return runnable(&path).then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(command))
        .find(|candidate| runnable(candidate))
}

fn runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    // On Unix a non-executable file on `PATH` is common (data, README); treating it as a command would register a tool that fails on first use.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
