//! One connection to a language server: handshake, request/response, and a clean death.
//! Four invariants: no query before the handshake; a dead server fails every pending call
//! at once; every server request is answered; and "busy" comes from `$/progress`, not guesswork.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

use crate::error::LspError;
use crate::launch::Channel;
use crate::proto;

/// A diagnostic exactly as the server sent it: 0-based, columns in UTF-16 code units; converted only where the file is at hand, since disk reads must not stall the reader task.
#[derive(Clone, Debug)]
pub struct RawNote {
    pub line: u32,
    pub character: u32,
    pub severity: u64,
    pub source: Option<String>,
    pub message: String,
}

type Sink = Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;
type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, LspError>>>>>;

/// What the reader task and [`Client`] both see.
struct State {
    label: String,
    alive: AtomicBool,
    /// Why it died; it is read in error messages, so it must be a sentence rather than a code.
    cause: Mutex<String>,
    /// `uri` -> (publish sequence, diagnostics); the sequence separates "server said nothing" from "server said there are no errors".
    diagnostics: Mutex<HashMap<String, (u64, Vec<RawNote>)>>,
    stamp: AtomicI64,
    fresh: Notify,
    /// Open `$/progress` tokens.
    busy: Mutex<HashSet<String>>,
}

impl State {
    fn die(&self, reason: String, pending: &Pending) {
        if self.alive.swap(false, Ordering::SeqCst) {
            *self.cause.lock() = reason.clone();
        }
        let taken: Vec<_> = { pending.lock().drain().map(|(_, tx)| tx).collect() };
        for tx in taken {
            let _ = tx.send(Err(LspError::Dead(self.label.clone(), reason.clone())));
        }
        // Wake the diagnostics waiters too: they are waiting for something that will never arrive.
        self.fresh.notify_waiters();
    }
}

/// A document we have opened with the server.
struct Doc {
    version: i64,
    /// Content fingerprint of the last send; the file changes between questions, and a server holding the old copy would answer about code that no longer exists.
    digest: u64,
}

pub struct Client {
    label: String,
    sink: Sink,
    pending: Pending,
    state: Arc<State>,
    next_id: AtomicI64,
    ready: AtomicBool,
    docs: tokio::sync::Mutex<HashMap<String, Doc>>,
    child: Mutex<Option<tokio::process::Child>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl Client {
    /// Build the client and start reading. No handshake yet - see [`Client::handshake`].
    pub fn start(label: impl Into<String>, channel: Channel) -> Arc<Client> {
        let label = label.into();
        let Channel {
            reader,
            writer,
            child,
        } = channel;

        let state = Arc::new(State {
            label: label.clone(),
            alive: AtomicBool::new(true),
            cause: Mutex::new(String::new()),
            diagnostics: Mutex::new(HashMap::new()),
            stamp: AtomicI64::new(0),
            fresh: Notify::new(),
            busy: Mutex::new(HashSet::new()),
        });
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(writer));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let handle = tokio::spawn(pump(
            reader,
            sink.clone(),
            pending.clone(),
            Arc::clone(&state),
        ));

        Arc::new(Client {
            label,
            sink,
            pending,
            state,
            next_id: AtomicI64::new(1),
            ready: AtomicBool::new(false),
            docs: tokio::sync::Mutex::new(HashMap::new()),
            child: Mutex::new(child),
            reader: Mutex::new(Some(handle)),
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn alive(&self) -> bool {
        self.state.alive.load(Ordering::SeqCst)
    }

    /// Is the server still doing something long (loading a workspace, indexing)?
    pub fn busy(&self) -> bool {
        !self.state.busy.lock().is_empty()
    }

    fn dead(&self) -> LspError {
        LspError::Dead(self.label.clone(), self.state.cause.lock().clone())
    }

    /// `initialize` -> `initialized`. Only after this does the client accept queries.
    pub async fn handshake(
        &self,
        root: &std::path::Path,
        root_uri: &str,
        options: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "private-ai-harness", "version": env!("CARGO_PKG_VERSION") },
            "rootUri": root_uri,
            // `rootPath` was deprecated long ago but some servers read only it; sending both is cheaper than detecting which.
            "rootPath": root.display().to_string(),
            "workspaceFolders": [{
                "uri": root_uri,
                "name": root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".into()),
            }],
            "capabilities": {
                "workspace": {
                    // Declaring `true` and answering `null` means "use your defaults"; declaring `false` silently degrades some servers.
                    "configuration": true,
                    "workspaceFolders": true,
                },
                "textDocument": {
                    "synchronization": { "didSave": false, "willSave": false },
                    "definition": { "linkSupport": true },
                    "references": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "publishDiagnostics": { "relatedInformation": false },
                },
                // This is what enables `$/progress`, and `$/progress` is the whole basis for saying "still indexing" instead of "nothing found".
                "window": { "workDoneProgress": true },
            },
            "initializationOptions": options,
        });

        let result = self.send_request("initialize", params, timeout).await?;
        self.send_notify("initialized", json!({})).await?;
        self.ready.store(true, Ordering::SeqCst);
        Ok(result)
    }

    /// One query. Refused before the handshake completes - see invariant 1 at the top of the file.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        if !self.ready.load(Ordering::SeqCst) {
            // Our bug, not the server's: something in the harness asked too early, and the two are fixed in different places.
            return Err(LspError::Protocol(format!(
                "harness gửi `{method}` tới `{}` trước khi bắt tay xong",
                self.label
            )));
        }
        self.send_request(method, params, timeout).await
    }

    async fn send_request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        if !self.alive() {
            return Err(self.dead());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);

        let mut message = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null()
            && let Some(object) = message.as_object_mut()
        {
            object.insert("params".into(), params);
        }
        if let Err(err) = self.write(&message).await {
            self.pending.lock().remove(&id);
            return Err(err);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            // The sender was dropped without sending: only happens when the reader task exits mid-flight.
            Ok(Err(_)) => Err(self.dead()),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err(LspError::Timeout(self.label.clone(), timeout))
            }
        }
    }

    async fn send_notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn write(&self, message: &Value) -> Result<(), LspError> {
        let mut sink = self.sink.lock().await;
        match proto::write_message(&mut *sink, message).await {
            Ok(()) => Ok(()),
            Err(err) => {
                // A failed write means the far end is closed; declare death here rather than waiting for the reader task to notice.
                self.state.die(err.to_string(), &self.pending);
                Err(self.dead())
            }
        }
    }

    /// Make sure the server is holding this file's current contents.
    pub async fn sync_document(
        &self,
        uri: &str,
        language_id: &str,
        text: &str,
    ) -> Result<(), LspError> {
        let digest = fingerprint(text);
        // Async lock held across the send: two overlapping queries on one file would otherwise both send `didOpen` and give the server two copies.
        let mut docs = self.docs.lock().await;
        match docs.get(uri) {
            None => {
                self.send_notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": {
                        "uri": uri, "languageId": language_id, "version": 1, "text": text,
                    }}),
                )
                .await?;
                docs.insert(uri.to_string(), Doc { version: 1, digest });
            }
            Some(doc) if doc.digest != digest => {
                let version = doc.version + 1;
                self.send_notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": version },
                        // Replace the whole file rather than sending a delta: we track disk state, not keystrokes, over a local pipe.
                        "contentChanges": [{ "text": text }],
                    }),
                )
                .await?;
                docs.insert(uri.to_string(), Doc { version, digest });
            }
            Some(_) => {}
        }
        Ok(())
    }

    /// Sequence number of the latest diagnostics publish for `uri`, or `0`.
    pub fn diagnostics_stamp(&self, uri: &str) -> u64 {
        self.state
            .diagnostics
            .lock()
            .get(uri)
            .map(|(stamp, _)| *stamp)
            .unwrap_or(0)
    }

    /// The latest publish, without waiting. `None` means the server never spoke about this file.
    pub fn diagnostics(&self, uri: &str) -> Option<Vec<RawNote>> {
        self.state
            .diagnostics
            .lock()
            .get(uri)
            .map(|(_, notes)| notes.clone())
    }

    /// Wait for a publish newer than `since`. `None` means the wait timed out.
    pub async fn wait_diagnostics(
        &self,
        uri: &str,
        since: u64,
        timeout: Duration,
    ) -> Option<Vec<RawNote>> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Register the waiter *before* looking, or a publish slipping between the two steps is lost and we wait out the timeout.
            let waiter = self.state.fresh.notified();
            if let Some((stamp, notes)) = self.state.diagnostics.lock().get(uri)
                && *stamp > since
            {
                return Some(notes.clone());
            }
            if !self.alive() {
                return None;
            }
            if tokio::time::timeout_at(deadline, waiter).await.is_err() {
                return None;
            }
        }
    }

    /// `shutdown` -> `exit` -> kill if needed; three steps because each catches a different kind of server.
    pub async fn shutdown(&self) {
        if self.alive() {
            let _ = self
                .send_request("shutdown", Value::Null, Duration::from_secs(3))
                .await;
            let _ = self.send_notify("exit", Value::Null).await;
        }
        if let Some(handle) = self.reader.lock().take() {
            handle.abort();
        }
        let child = self.child.lock().take();
        if let Some(mut child) = child {
            let quit = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            if quit.is_err() {
                tracing::warn!(server = %self.label, "language server did not exit after `exit`; killing it");
                let _ = child.kill().await;
            }
        }
        self.state.die("đã đóng theo yêu cầu".into(), &self.pending);
    }
}

/// FNV-1a; it only answers "did the file change between two questions", so a cryptographic dependency would be paying for something unused.
fn fingerprint(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Reader task: one message per iteration, until the pipe closes.
async fn pump(
    reader: Box<dyn AsyncRead + Send + Unpin>,
    sink: Sink,
    pending: Pending,
    state: Arc<State>,
) {
    let mut source = BufReader::new(reader);
    loop {
        match proto::read_message(&mut source).await {
            Ok(Some(message)) => dispatch(message, &sink, &pending, &state).await,
            Ok(None) => {
                state.die("ống stdout đã đóng".into(), &pending);
                return;
            }
            Err(err) => {
                state.die(format!("đọc hỏng: {err}"), &pending);
                return;
            }
        }
    }
}

async fn dispatch(message: Value, sink: &Sink, pending: &Pending, state: &Arc<State>) {
    let id = message.get("id").and_then(Value::as_i64);
    let method = message.get("method").and_then(Value::as_str);

    match (id, method) {
        // A reply to something we asked.
        (Some(id), None) => {
            let Some(tx) = pending.lock().remove(&id) else {
                tracing::debug!(server = %state.label, id, "reply for a query that was already abandoned");
                return;
            };
            let outcome = match message.get("error") {
                Some(error) => Err(LspError::Protocol(describe(error))),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(outcome);
        }
        // The server is asking us. Always answer - see invariant 3.
        (Some(id), Some(method)) => {
            let result = answer(method, &message, state);
            let reply = match result {
                Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                None => json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("harness không cài `{method}`") },
                }),
            };
            let mut guard = sink.lock().await;
            if let Err(err) = proto::write_message(&mut *guard, &reply).await {
                drop(guard);
                state.die(format!("không trả lời được server: {err}"), pending);
            }
        }
        (None, Some(method)) => notice(method, &message, state),
        (None, None) => {
            tracing::debug!(server = %state.label, "message has neither `id` nor `method`")
        }
    }
}

/// Answer a server request. `None` = we do not implement that method.
fn answer(method: &str, message: &Value, state: &Arc<State>) -> Option<Value> {
    match method {
        // The server wants a progress token; record it here rather than at `$/progress` `begin`, since we are often asked in exactly that gap.
        "window/workDoneProgress/create" => {
            if let Some(token) = message.pointer("/params/token") {
                state.busy.lock().insert(token_key(token));
            }
            Some(Value::Null)
        }
        // `null` per item means "no specific configuration, use defaults"; the harness has no per-server config file and must not invent one.
        "workspace/configuration" => {
            let count = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            Some(Value::Array(vec![Value::Null; count]))
        }
        "workspace/workspaceFolders" => Some(Value::Null),
        // We have no dynamic capability registry; accepting and ignoring is right, since every method we call is static in the spec.
        "client/registerCapability" | "client/unregisterCapability" => Some(Value::Null),
        "window/showMessageRequest" => Some(Value::Null),
        _ => None,
    }
}

fn notice(method: &str, message: &Value, state: &Arc<State>) {
    match method {
        "textDocument/publishDiagnostics" => {
            let Some(uri) = message
                .pointer("/params/uri")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                return;
            };
            let notes = message
                .pointer("/params/diagnostics")
                .and_then(Value::as_array)
                .map(|items| items.iter().map(raw_note).collect())
                .unwrap_or_default();
            let stamp = state.stamp.fetch_add(1, Ordering::Relaxed) as u64 + 1;
            state.diagnostics.lock().insert(uri, (stamp, notes));
            state.fresh.notify_waiters();
        }
        "$/progress" => {
            let Some(token) = message.pointer("/params/token") else {
                return;
            };
            let key = token_key(token);
            match message
                .pointer("/params/value/kind")
                .and_then(Value::as_str)
            {
                Some("begin") => {
                    state.busy.lock().insert(key);
                }
                Some("end") => {
                    state.busy.lock().remove(&key);
                }
                _ => {}
            }
        }
        "window/logMessage" | "window/showMessage" => {
            if let Some(text) = message.pointer("/params/message").and_then(Value::as_str) {
                tracing::debug!(server = %state.label, "{text}");
            }
        }
        _ => tracing::trace!(server = %state.label, method, "notification ignored"),
    }
}

/// A progress token is `string | integer`; normalize to a string so one `HashSet` suffices.
fn token_key(token: &Value) -> String {
    match token {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn raw_note(item: &Value) -> RawNote {
    RawNote {
        line: item
            .pointer("/range/start/line")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        character: item
            .pointer("/range/start/character")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        // The spec allows a missing `severity`, leaving it to the client; treating it as an error is the safe direction.
        severity: item.get("severity").and_then(Value::as_u64).unwrap_or(1),
        source: item
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_string),
        message: item
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("(không có mô tả)")
            .to_string(),
    }
}

fn describe(error: &Value) -> String {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("lỗi không có mô tả");
    match error.get("code").and_then(Value::as_i64) {
        Some(code) => format!("{message} (mã {code})"),
        None => message.to_string(),
    }
}
