//! `McpHub` — every third-party server behind one object.
//! Best-effort: one server per supervisor task, so a failure stops there. One owner per
//! connection. Hot-reload that leaves unchanged servers connected — see [`McpHub::reload`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pai_core::Guard;
use pai_tools::{Resolution, ToolRegistry};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{CONNECT_TIMEOUT, ConfigError, DEFAULT_MAX_RETRIES, ServerConfig};
use crate::dial::{ConfigDialers, Dialer, DialerFactory, Reach};
use crate::naming::qualify;
use crate::remote::{Link, RemoteTool};

/// A connection that lasts this long counts as healthy, so its next drop resets the attempt counter.
const HEALTHY_AFTER: Duration = Duration::from_secs(30);

/// A server's state, enough for the UI to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerState {
    Connecting,
    Ready {
        tools: usize,
    },
    /// Out of attempts; the server stays listed so the user can see it failed.
    Failed {
        reason: String,
    },
    Stopped,
}

/// One server row for the UI: how many tools mounted, under what names, and why it failed if it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    pub name: String,
    pub state: ServerState,
    /// Fully prefixed names, `ext.<server>.<tool>`: the user must see what the model actually calls.
    pub tools: Vec<String>,
    /// The most recent failure reason, kept even after a successful reconnect, or a flaky server leaves no trace.
    pub error: Option<String>,
}

/// The result of mounting one server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mount {
    Connected {
        tools: usize,
    },
    /// The first dial failed; not an error, since the supervisor keeps retrying.
    Unavailable {
        reason: String,
    },
}

/// How long, and how many times.
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub connect_timeout: Duration,
    pub max_retries: u32,
}

impl Default for RetryPolicy {
    fn default() -> RetryPolicy {
        RetryPolicy {
            connect_timeout: CONNECT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

/// Where the supervisor writes what [`McpHub::status`] reads; a locked cell, since nobody awaits it.
#[derive(Default)]
struct Report {
    /// Already prefixed, exactly as in the registry.
    tools: Mutex<Vec<String>>,
    error: Mutex<Option<String>>,
}

struct Mounted {
    /// A config snapshot so [`McpHub::reload`] sees changes; `None` for a hand-mounted dialer, which always counts as changed.
    fingerprint: Option<String>,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
    state: watch::Receiver<ServerState>,
    report: Arc<Report>,
}

/// Every third-party server the application is talking to.
pub struct McpHub {
    registry: Arc<ToolRegistry>,
    servers: Mutex<HashMap<String, Mounted>>,
    dialers: Arc<dyn DialerFactory>,
}

impl McpHub {
    pub fn new(registry: Arc<ToolRegistry>) -> Arc<McpHub> {
        McpHub::with_dialers(registry, Arc::new(ConfigDialers))
    }

    /// The same hub with a different way to dial. See [`DialerFactory`].
    pub fn with_dialers(
        registry: Arc<ToolRegistry>,
        dialers: Arc<dyn DialerFactory>,
    ) -> Arc<McpHub> {
        Arc::new(McpHub {
            registry,
            servers: Mutex::new(HashMap::new()),
            dialers,
        })
    }

    /// Mount a server from the user's config; `Err` is only for bad config, an unreachable server is `Ok(Unavailable)`.
    pub async fn mount(&self, config: ServerConfig) -> Result<Mount, ConfigError> {
        config.validate()?;
        let policy = RetryPolicy {
            connect_timeout: config.connect_timeout(),
            max_retries: config.max_retries,
        };
        let name = config.name.clone();
        let fingerprint = fingerprint(&config);
        let dialer = self.dialers.make(&config);
        Ok(self.install(name, dialer, policy, Some(fingerprint)).await)
    }

    /// Mount using a hand-built [`Dialer`]; this is the door tests come in through.
    pub async fn mount_dialer(
        &self,
        name: impl Into<String>,
        dialer: Arc<dyn Dialer>,
        policy: RetryPolicy,
    ) -> Mount {
        self.install(name.into(), dialer, policy, None).await
    }

    async fn install(
        &self,
        name: String,
        dialer: Arc<dyn Dialer>,
        policy: RetryPolicy,
        fingerprint: Option<String>,
    ) -> Mount {
        // Mounting over an existing name replaces it: two supervisors would fight over the same tool names.
        self.unmount(&name).await;

        let cancel = CancellationToken::new();
        let (state_tx, state_rx) = watch::channel(ServerState::Connecting);
        let (first_tx, first_rx) = oneshot::channel();
        let link = Link::new();
        let report = Arc::new(Report::default());

        let supervisor = Supervisor {
            name: name.clone(),
            dialer,
            registry: self.registry.clone(),
            link,
            state: state_tx,
            report: report.clone(),
            ct: cancel.clone(),
            policy,
        };
        let handle = tokio::spawn(supervisor.run(Some(first_tx)));

        self.servers.lock().insert(
            name.clone(),
            Mounted {
                fingerprint,
                cancel,
                handle,
                state: state_rx,
                report,
            },
        );

        // Wait for the first attempt only, with our own deadline: a third-party `Dialer` need not honour its own.
        let deadline = policy.connect_timeout.saturating_mul(2) + Duration::from_secs(1);
        match tokio::time::timeout(deadline, first_rx).await {
            Ok(Ok(mount)) => mount,
            Ok(Err(_)) => Mount::Unavailable {
                reason: "task giám sát dừng trước khi báo kết quả".into(),
            },
            Err(_) => Mount::Unavailable {
                reason: format!("`{name}` không trả lời trong {deadline:?}"),
            },
        }
    }

    /// Unmount a server: the supervisor task owns the registration guards, so ending it clears the registry.
    pub async fn unmount(&self, name: &str) -> bool {
        let Some(mounted) = self.servers.lock().remove(name) else {
            return false;
        };
        mounted.cancel.cancel();
        if let Err(err) = mounted.handle.await {
            tracing::warn!(server = %name, %err, "the MCP supervisor task ended abnormally");
        }
        true
    }

    /// Reset the whole server list without touching unchanged servers; returns a result for each one actually touched.
    pub async fn reload(
        &self,
        configs: Vec<ServerConfig>,
    ) -> Vec<(String, Result<Mount, ConfigError>)> {
        let mut wanted: HashMap<String, ServerConfig> = HashMap::new();
        let mut report: Vec<(String, Result<Mount, ConfigError>)> = Vec::new();

        for config in configs {
            if let Err(err) = config.validate() {
                report.push((config.name.clone(), Err(err)));
                continue;
            }
            if !config.enabled {
                continue;
            }
            wanted.insert(config.name.clone(), config);
        }

        let obsolete: Vec<String> = {
            let servers = self.servers.lock();
            servers
                .iter()
                .filter(|(name, mounted)| match wanted.get(*name) {
                    None => true,
                    Some(config) => mounted.fingerprint.as_deref() != Some(&fingerprint(config)),
                })
                .map(|(name, _)| name.clone())
                .collect()
        };
        for name in obsolete {
            self.unmount(&name).await;
        }

        for (name, config) in wanted {
            if self.servers.lock().contains_key(&name) {
                continue;
            }
            report.push((name, self.mount(config).await));
        }
        report
    }

    /// Close everything; used when the plugin is torn down.
    pub async fn shutdown(&self) {
        let names: Vec<String> = self.servers.lock().keys().cloned().collect();
        for name in names {
            self.unmount(&name).await;
        }
    }

    pub fn servers(&self) -> Vec<String> {
        let mut names: Vec<String> = self.servers.lock().keys().cloned().collect();
        names.sort();
        names
    }

    pub fn state(&self, name: &str) -> Option<ServerState> {
        Some(self.servers.lock().get(name)?.state.borrow().clone())
    }

    /// A name-sorted snapshot of mounted servers only; "disabled" belongs to the config store, see [`crate::store`].
    pub fn status(&self) -> Vec<ServerStatus> {
        let mut out: Vec<ServerStatus> = {
            let servers = self.servers.lock();
            servers
                .iter()
                .map(|(name, mounted)| ServerStatus {
                    name: name.clone(),
                    state: mounted.state.borrow().clone(),
                    tools: mounted.report.tools.lock().clone(),
                    error: mounted.report.error.lock().clone(),
                })
                .collect()
        };
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

fn fingerprint(config: &ServerConfig) -> String {
    // The config always serialises; a failure only empties the fingerprint, which costs one extra reconnect.
    serde_json::to_string(config).unwrap_or_default()
}

/// Register remote tools, returning one guard each plus the prefixed names that actually made it into the registry.
fn register_tools(
    registry: &Arc<ToolRegistry>,
    server: &str,
    tools: &[rmcp::model::Tool],
    link: &Arc<Link>,
    reach: Reach,
) -> (Vec<Guard>, Vec<String>) {
    let mut guards = Vec::new();
    let mut names = Vec::new();
    for tool in tools {
        // The prefix is applied here, the first place a remote name touches us: before the registry, before the log.
        let name = qualify(server, &tool.name);

        if !name.round_trips() {
            tracing::warn!(
                server = %server, tool = %tool.name,
                "skipped: the name contains `__`, so the wire encoding is not reversible"
            );
            continue;
        }

        // The prefix rules out internal clashes; this guards the rest, and shadowing is costly enough to check twice.
        if !matches!(
            registry.resolve(None, name.as_str()),
            Resolution::Unknown(_)
        ) {
            tracing::warn!(tool = %name, "skipped: a tool of that name is already registered");
            continue;
        }

        let parameters = Value::Object((*tool.input_schema).clone());
        names.push(name.as_str().to_string());
        guards.push(registry.register(Arc::new(RemoteTool::new(
            name,
            tool.name.to_string(),
            server,
            tool.description.clone().unwrap_or_default().to_string(),
            parameters,
            link.clone(),
            reach,
        ))));
    }
    (guards, names)
}

fn backoff(attempt: u32) -> Duration {
    // 1s, 2s, 4s, 8s, 16s and no further: unbounded exponential backoff lands the tenth attempt a day later.
    Duration::from_secs(1u64 << (attempt.clamp(1, 5) - 1))
}

/// The task owning one server's connection; every field outlives the dial-register-wait-drop-redial cycle, unlike `first`.
struct Supervisor {
    name: String,
    dialer: Arc<dyn Dialer>,
    registry: Arc<ToolRegistry>,
    link: Arc<Link>,
    state: watch::Sender<ServerState>,
    report: Arc<Report>,
    ct: CancellationToken,
    policy: RetryPolicy,
}

impl Supervisor {
    async fn run(self, mut first: Option<oneshot::Sender<Mount>>) {
        let mut attempt: u32 = 0;

        loop {
            if self.ct.is_cancelled() {
                break;
            }
            let _ = self.state.send(ServerState::Connecting);

            // A child token per connection: sharing one would leave the next redial starting from an already-cancelled token.
            let conn_ct = self.ct.child_token();
            let dialed = tokio::select! {
                () = self.ct.cancelled() => break,
                result = tokio::time::timeout(
                    self.policy.connect_timeout,
                    self.dialer.dial(conn_ct),
                ) => result,
            };

            let service = match dialed {
                Ok(Ok(service)) => service,
                Ok(Err(err)) => {
                    if !self.retry(&mut attempt, &mut first, err.to_string()).await {
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    let reason = format!(
                        "hết {:?} mà chưa xong initialize",
                        self.policy.connect_timeout
                    );
                    if !self.retry(&mut attempt, &mut first, reason).await {
                        break;
                    }
                    continue;
                }
            };

            let tools = match service.peer().list_all_tools().await {
                Ok(tools) => tools,
                Err(err) => {
                    // Open but useless; close it before retrying, or every round leaks a child process.
                    let _ = service.cancel().await;
                    let reason = format!("không đọc được danh sách tool: {err}");
                    if !self.retry(&mut attempt, &mut first, reason).await {
                        break;
                    }
                    continue;
                }
            };

            self.link.set(service.peer().clone());
            let (guards, names) = register_tools(
                &self.registry,
                &self.name,
                &tools,
                &self.link,
                self.dialer.reach(),
            );
            let count = guards.len();
            *self.report.tools.lock() = names;
            tracing::info!(server = %self.name, tools = count, "MCP server ready");
            let _ = self.state.send(ServerState::Ready { tools: count });
            settle(&mut first, Mount::Connected { tools: count });

            // `waiting()` takes `self` by value, so a child task owns the connection while we stay able to `select!`.
            let started = Instant::now();
            let mut watcher = tokio::spawn(async move { service.waiting().await });
            let stopped_by_us = tokio::select! {
                () = self.ct.cancelled() => true,
                _ = &mut watcher => false,
            };
            if stopped_by_us {
                // Wait for the owning task to finish cleaning up, or a stdio child outlives the user's click.
                let _ = watcher.await;
            }

            // Drop the guards before retrying: with no connection, none of these tools may stay advertised.
            drop(guards);
            self.link.clear();
            // The tool list empties exactly when the registry does, or the UI offers a tool that just vanished.
            self.report.tools.lock().clear();

            if self.ct.is_cancelled() {
                break;
            }
            // Long enough alive means this drop is an incident, not a server broken from the start.
            if started.elapsed() >= HEALTHY_AFTER {
                attempt = 0;
            }
            if !self
                .retry(&mut attempt, &mut first, "kết nối đứt".into())
                .await
            {
                break;
            }
        }

        self.link.clear();
        // `Stopped` must not overwrite `Failed`: "I gave up, here is why" and "you told me to stop" are different answers.
        let gave_up = matches!(*self.state.borrow(), ServerState::Failed { .. });
        if !gave_up {
            let _ = self.state.send(ServerState::Stopped);
        }
    }

    /// `false` means stop trying.
    async fn retry(
        &self,
        attempt: &mut u32,
        first: &mut Option<oneshot::Sender<Mount>>,
        reason: String,
    ) -> bool {
        tracing::warn!(server = %self.name, %reason, "MCP server unusable");
        // Record the reason now and never clear it on a later success: the question is always asked afterwards.
        *self.report.error.lock() = Some(reason.clone());
        // Settle the first result before sleeping: `mount` is waiting, and so is the application window.
        settle(
            first,
            Mount::Unavailable {
                reason: reason.clone(),
            },
        );

        *attempt += 1;
        if *attempt > self.policy.max_retries {
            tracing::warn!(server = %self.name, "giving up on reconnecting to the MCP server");
            let _ = self.state.send(ServerState::Failed { reason });
            return false;
        }

        let delay = backoff(*attempt);
        tokio::select! {
            () = self.ct.cancelled() => false,
            () = tokio::time::sleep(delay) => true,
        }
    }
}

fn settle(first: &mut Option<oneshot::Sender<Mount>>, mount: Mount) {
    if let Some(tx) = first.take() {
        let _ = tx.send(mount);
    }
}
