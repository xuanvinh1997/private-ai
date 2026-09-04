//! Local session registry: the on-machine implementation of the seam.
//! Sessions belong to their creator, and another owner's id fails exactly like an unknown one.
//! The sandbox is resolved from `Context` at open time, and a broken wrapper refuses to open.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::Context;
use pai_sandbox::{Policy, Sandbox};
use parking_lot::Mutex;

use crate::buffer::Page;
use crate::seam::{
    DEFAULT_COLS, DEFAULT_MAX_LINES, DEFAULT_ROWS, OpenRequest, Owner, Sent, SessionInfo, Signal,
    Stop, TerminalError, TerminalHost, Wait,
};
use crate::session::{Session, Spec};

/// Poll interval while waiting for a command to go quiet: fast enough for short commands, slow enough not to busy-loop.
const SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// The only backend. A table lookup rather than a `command` parameter, so the approval prompt has exactly one meaning.
pub const SHELL_BACKEND: &str = "shell";

pub struct LocalTerminals {
    ctx: Context,
    policy: Policy,
    cwd: PathBuf,
    max_lines: usize,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl LocalTerminals {
    pub fn new(ctx: Context, policy: Policy, cwd: PathBuf) -> LocalTerminals {
        LocalTerminals {
            ctx,
            policy,
            cwd,
            max_lines: DEFAULT_MAX_LINES,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_max_lines(mut self, lines: usize) -> LocalTerminals {
        self.max_lines = lines;
        self
    }

    /// argv after sandbox wrapping, if anything wraps it. See the module header.
    fn argv(&self) -> Result<Vec<String>, TerminalError> {
        // `-i` rather than a command: we are opening a session, and only an interactive shell remembers an earlier `cd`.
        let argv = vec!["/bin/sh".to_string(), "-i".to_string()];
        match self.ctx.get::<Sandbox>() {
            Some(sandbox) => sandbox
                .wrap(argv, &self.policy)
                .map_err(|err| TerminalError::Spawn(err.to_string())),
            None => Ok(argv),
        }
    }

    /// Look up a session belonging to this exact owner.
    fn find(&self, owner: Owner, id: &str) -> Result<Arc<Session>, TerminalError> {
        let found = self.sessions.lock().get(id).cloned();
        match found {
            Some(session) if session.owner == owner => Ok(session),
            // The remaining two cases stay merged. See the module header.
            _ => Err(TerminalError::NoSession(id.to_string())),
        }
    }
}

#[async_trait]
impl TerminalHost for LocalTerminals {
    async fn open(&self, owner: Owner, req: OpenRequest) -> Result<SessionInfo, TerminalError> {
        if req.backend != SHELL_BACKEND {
            return Err(TerminalError::NoBackend(
                req.backend,
                SHELL_BACKEND.to_string(),
            ));
        }

        let id = uuid::Uuid::now_v7().to_string();
        let name = req.name.unwrap_or_else(|| {
            // Always a name, even unnamed: the session list is read by humans and a blank column says nothing.
            format!("{}-{}", SHELL_BACKEND, &id[..8.min(id.len())])
        });
        let cwd = req.cwd.unwrap_or_else(|| self.cwd.clone());

        let session = Session::open(
            Spec {
                id: id.clone(),
                name,
                backend: SHELL_BACKEND.to_string(),
                owner,
                cwd,
                max_lines: self.max_lines,
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
            },
            &self.argv()?,
        )?;
        session.prime().await;

        let info = session.info();
        self.sessions.lock().insert(id, session);
        Ok(info)
    }

    fn list(&self, owner: Owner) -> Vec<SessionInfo> {
        let mut rows: Vec<SessionInfo> = self
            .sessions
            .lock()
            .values()
            .filter(|session| session.owner == owner)
            .map(|session| session.info())
            .collect();
        // Ids are UUIDv7, so sorting by id sorts by open order and "the second one" stays meaningful across calls.
        rows.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    fn info(&self, owner: Owner, id: &str) -> Result<SessionInfo, TerminalError> {
        Ok(self.find(owner, id)?.info())
    }

    async fn send(
        &self,
        owner: Owner,
        id: &str,
        bytes: &[u8],
        wait: Option<Wait>,
    ) -> Result<Sent, TerminalError> {
        let session = self.find(owner, id)?;
        if !session.alive() {
            return Err(TerminalError::Ended(id.to_string()));
        }

        // Take the mark before writing: taking it after loses output produced in between, worst for the fastest commands.
        let mark = session.mark();
        session.write(bytes)?;

        let Some(wait) = wait else {
            return Ok(Sent {
                lines: Vec::new(),
                dropped: session.dropped(),
                stopped: Stop::Background,
            });
        };

        let started = std::time::Instant::now();
        let mut last_seen = session.mark();
        let mut last_change = started;
        let stopped = loop {
            tokio::time::sleep(SETTLE_POLL).await;
            let now = session.mark();
            if now != last_seen {
                last_seen = now;
                last_change = std::time::Instant::now();
            }
            if !session.alive() {
                break Stop::Ended;
            }
            if last_change.elapsed() >= wait.quiet {
                break Stop::Quiet;
            }
            if started.elapsed() >= wait.timeout {
                break Stop::Timeout;
            }
        };

        Ok(Sent {
            lines: session.since(mark),
            dropped: session.dropped(),
            stopped,
        })
    }

    fn read(
        &self,
        owner: Owner,
        id: &str,
        offset: usize,
        count: usize,
    ) -> Result<Page, TerminalError> {
        // Dead sessions stay readable on purpose: their last output is usually the most useful.
        Ok(self.find(owner, id)?.read(offset, count))
    }

    fn resize(&self, owner: Owner, id: &str, rows: u16, cols: u16) -> Result<(), TerminalError> {
        self.find(owner, id)?.resize(rows, cols)
    }

    fn signal(&self, owner: Owner, id: &str, signal: Signal) -> Result<(), TerminalError> {
        self.find(owner, id)?.signal(signal)
    }

    async fn close(&self, owner: Owner, id: &str) -> Result<(), TerminalError> {
        let session = self.find(owner, id)?;
        session.close().await;
        // Drop from the registry only after the process tree is gone, so a slow close cannot report the id as missing while it runs.
        self.sessions.lock().remove(id);
        Ok(())
    }

    async fn close_all(&self) {
        let all: Vec<Arc<Session>> = self.sessions.lock().drain().map(|(_, s)| s).collect();
        for session in all {
            session.close().await;
        }
    }
}
