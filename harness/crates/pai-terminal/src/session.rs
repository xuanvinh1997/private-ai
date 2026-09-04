//! One PTY session: the shell, the ring buffer, and the process tree behind it.
//! What we run is a process tree, not a process, and an interactive shell's job control would put each
//! background job in its own group -- so the session is primed with `set +m` and signalled group-wide.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::buffer::{Page, Ring};
use crate::seam::{Owner, SessionInfo, Signal, TerminalError};

/// How long the shell gets to print its first prompt before we wipe the priming; a trade-off, not a correct constant.
const PRIME_SETTLE: Duration = Duration::from_millis(250);

/// How long the process tree gets to exit on its own before we force it.
const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// Poll interval for "has it exited yet".
const POLL: Duration = Duration::from_millis(25);

pub struct Session {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub cwd: PathBuf,
    pub owner: Owner,
    /// The session's process group; on Unix `portable-pty` calls `setsid()` in the child, so pid is the pgid.
    pid: Option<u32>,
    master: Mutex<Box<dyn MasterPty>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    size: Mutex<PtySize>,
    buffer: Arc<Mutex<Ring>>,
}

/// Everything needed to open a session; a struct, because nine same-typed parameters are nine chances to swap two.
pub struct Spec {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub owner: Owner,
    pub cwd: PathBuf,
    pub max_lines: usize,
    pub rows: u16,
    pub cols: u16,
}

impl Session {
    /// Open a session. `argv` is already sandbox-wrapped by [`crate::provider`], the only place holding a `Context`.
    pub fn open(spec: Spec, argv: &[String]) -> Result<Arc<Session>, TerminalError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| TerminalError::Spawn("argv rỗng".into()))?;
        let cwd: &Path = &spec.cwd;

        let size = PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;

        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        // A meaningful `TERM` is the other half of "a real terminal": without it curses libraries still refuse to draw.
        command.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;
        // Drop the slave end now: while we hold it the master never sees EOF and the pump thread waits forever.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;

        let buffer = Arc::new(Mutex::new(Ring::new(spec.max_lines)));
        {
            // Reading a PTY is an uncancellable blocking call, so it gets an OS thread rather than a shared blocking-pool slot.
            let sink = buffer.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                while let Ok(read) = reader.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    sink.lock().push(&chunk[..read]);
                }
            });
        }

        let session = Arc::new(Session {
            id: spec.id,
            name: spec.name,
            backend: spec.backend,
            cwd: spec.cwd.clone(),
            owner: spec.owner,
            pid: child.process_id(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            size: Mutex::new(size),
            buffer,
        });
        Ok(session)
    }

    /// Turn job control off, then erase the trace of doing so. Separate from [`Session::open`] because it awaits.
    pub async fn prime(&self) {
        #[cfg(unix)]
        {
            if self.write(b"set +m\n").is_err() {
                return;
            }
        }
        tokio::time::sleep(PRIME_SETTLE).await;
        // The buffer starts here: a line the model did not type is a line it will try to explain.
        self.buffer.lock().reset();
    }

    pub fn info(&self) -> SessionInfo {
        let size = *self.size.lock();
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            backend: self.backend.clone(),
            cwd: self.cwd.display().to_string(),
            rows: size.rows,
            cols: size.cols,
            alive: self.alive(),
        }
    }

    pub fn alive(&self) -> bool {
        matches!(self.child.lock().try_wait(), Ok(None))
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), TerminalError> {
        let mut writer = self.writer.lock();
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|err| TerminalError::Write(err.to_string()))
    }

    pub fn read(&self, offset: usize, count: usize) -> Page {
        self.buffer.lock().page(offset, count)
    }

    /// Mark for asking "what is new since here".
    pub fn mark(&self) -> u64 {
        self.buffer.lock().produced()
    }

    pub fn since(&self, mark: u64) -> Vec<String> {
        self.buffer.lock().since(mark)
    }

    pub fn dropped(&self) -> usize {
        self.buffer.lock().dropped()
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), TerminalError> {
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .lock()
            .resize(size)
            .map_err(|err| TerminalError::Write(err.to_string()))?;
        *self.size.lock() = size;
        Ok(())
    }

    /// The foreground process group, and whether it is the session shell itself -- `SIGKILL` on the shell is refused.
    #[cfg(unix)]
    fn foreground(&self) -> (Option<u32>, bool) {
        let leader = self
            .master
            .lock()
            .process_group_leader()
            .map(|pid| pid as u32);
        match (leader, self.pid) {
            (Some(leader), Some(shell)) => (Some(leader), leader == shell),
            (None, shell) => (shell, true),
            (leader, None) => (leader, false),
        }
    }

    #[cfg(unix)]
    pub fn signal(&self, signal: Signal) -> Result<(), TerminalError> {
        let (target, is_shell) = self.foreground();
        if signal == Signal::Kill && is_shell {
            return Err(TerminalError::Refused(
                "SIGKILL nhắm vào chính shell của phiên bỏ lại một cây tiến trình không ai \
                 dọn; dùng `terminal_close`, nó có phần chờ cho cây tiến trình biến mất."
                    .into(),
            ));
        }
        let Some(target) = target else {
            return Err(TerminalError::Ended(self.id.clone()));
        };
        signal_group(target, raw_signal(signal));
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn signal(&self, _signal: Signal) -> Result<(), TerminalError> {
        Err(TerminalError::Refused(
            "gửi tín hiệu chưa làm được trên nền tảng này".into(),
        ))
    }

    /// Close the session and wait for its process tree to vanish; signals go to the whole group, as in `pai-shell`.
    pub async fn close(&self) {
        self.signal_all(sigterm());
        if self.wait_gone(CLOSE_GRACE).await {
            return;
        }
        self.signal_all(sigkill());
        // After `SIGKILL` this wait is just reaping, but it stays bounded in case a process is unkillable.
        self.wait_gone(CLOSE_GRACE).await;
        let _ = self.child.lock().kill();
    }

    fn signal_all(&self, signal: i32) {
        if let Some(pid) = self.pid {
            signal_group(pid, signal);
        }
    }

    /// `true` if the process exited within the budget.
    async fn wait_gone(&self, budget: Duration) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if !matches!(self.child.lock().try_wait(), Ok(None)) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

/// Signal a whole process group; the negative pid means "the group", which is the entire reason `set +m` is primed.
#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    unsafe { libc::kill(-(pid as i32), signal) };
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {
    // Windows has no process groups in this sense; the equivalent is a Job Object and belongs to `pai-sandbox`.
}

#[cfg(unix)]
fn raw_signal(signal: Signal) -> i32 {
    match signal {
        Signal::Int => libc::SIGINT,
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
        Signal::Tstp => libc::SIGTSTP,
        Signal::Hup => libc::SIGHUP,
    }
}

#[cfg(unix)]
fn sigterm() -> i32 {
    libc::SIGTERM
}
#[cfg(unix)]
fn sigkill() -> i32 {
    libc::SIGKILL
}
#[cfg(not(unix))]
fn sigterm() -> i32 {
    0
}
#[cfg(not(unix))]
fn sigkill() -> i32 {
    0
}
