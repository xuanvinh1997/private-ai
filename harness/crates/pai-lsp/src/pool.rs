//! Seam implementation: one child process per language, started on demand.
//! It is also the border between harness and LSP vocabulary: paths go through
//! [`FileRoots::resolve_read`], and the 1-based/UTF-16 conversion lives only at the bottom.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use pai_fs::FileRoots;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::client::{Client, RawNote};
use crate::config::{Limits, language_id};
use crate::error::LspError;
use crate::launch::Launch;
use crate::seam::{Answer, Hit, LanguageServers, Note, Operation, Query};
use crate::uri;

/// A config row whose command was located and is ready to run.
pub struct Entry {
    pub id: String,
    pub extensions: Vec<String>,
    pub launcher: Arc<dyn Launch>,
    pub options: Option<Value>,
}

/// State of one startup.
#[derive(Clone)]
enum Startup {
    Starting,
    Ready(Arc<Client>),
    Failed(String),
}

pub struct StdioServers {
    workspace: PathBuf,
    roots: FileRoots,
    entries: Vec<Entry>,
    limits: Limits,
    slots: Mutex<HashMap<String, watch::Receiver<Startup>>>,
}

impl StdioServers {
    pub fn new(
        workspace: PathBuf,
        roots: FileRoots,
        entries: Vec<Entry>,
        limits: Limits,
    ) -> StdioServers {
        StdioServers {
            // Canonicalize once here: server paths come back canonicalized, so an uncanonical root makes the display `strip_prefix` silently miss.
            workspace: workspace.canonicalize().unwrap_or(workspace),
            roots,
            entries,
            limits,
            slots: Mutex::new(HashMap::new()),
        }
    }

    fn entry_for(&self, path: &Path) -> Option<&Entry> {
        let extension = path.extension()?.to_str()?;
        self.entries
            .iter()
            .find(|entry| entry.extensions.iter().any(|ext| ext == extension))
    }

    /// Normalize first, check second. Relative paths resolve against the working directory.
    fn resolve(&self, path: &Path) -> Result<PathBuf, LspError> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        self.roots
            .resolve_read(&candidate)
            .map_err(|err| LspError::Invalid(err.to_string()))
    }

    /// Get a client that has finished its handshake, waiting with a deadline; a timeout is not a failure, since the startup task keeps running in the background.
    async fn ready_client(&self, entry: &Entry) -> Result<Arc<Client>, LspError> {
        let mut rx = self.slot(entry);
        let started = tokio::time::Instant::now();
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                Startup::Ready(client) => return Ok(client),
                Startup::Failed(reason) => {
                    return Err(LspError::Launch(entry.id.clone(), reason));
                }
                Startup::Starting => {}
            }
            let left = self.limits.startup.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(LspError::NotReady(entry.id.clone(), self.limits.startup));
            }
            if tokio::time::timeout(left, rx.changed()).await.is_err() {
                return Err(LspError::NotReady(entry.id.clone(), self.limits.startup));
            }
        }
    }

    /// The slot for a language, built when absent or when the old one died; a server that died after running is retried, one that never started is not.
    fn slot(&self, entry: &Entry) -> watch::Receiver<Startup> {
        let mut slots = self.slots.lock();
        if let Some(rx) = slots.get(&entry.id) {
            let usable = match &*rx.borrow() {
                Startup::Ready(client) => client.alive(),
                _ => true,
            };
            if usable {
                return rx.clone();
            }
        }

        let (tx, rx) = watch::channel(Startup::Starting);
        let launcher = entry.launcher.clone();
        let options = entry.options.clone();
        let root = self.workspace.clone();
        // The background task gets a longer deadline than the caller: the caller gives up early because it holds the user's turn, and cutting the task short would make "ask again in a few seconds" a lie.
        let timeout = self.limits.startup.saturating_mul(3);
        let label = entry.id.clone();
        tokio::spawn(async move {
            let outcome = boot(label, launcher, &root, options, timeout).await;
            let _ = tx.send(match outcome {
                Ok(client) => Startup::Ready(client),
                Err(err) => Startup::Failed(err.to_string()),
            });
        });
        slots.insert(entry.id.clone(), rx.clone());
        rx
    }

    /// Close every server. For plugin teardown.
    pub async fn shutdown(&self) {
        let slots: Vec<_> = { self.slots.lock().drain().map(|(_, rx)| rx).collect() };
        for rx in slots {
            let client = match &*rx.borrow() {
                Startup::Ready(client) => Some(client.clone()),
                _ => None,
            };
            if let Some(client) = client {
                client.shutdown().await;
            }
        }
    }
}

/// Start and handshake. Split from [`StdioServers`] so it can run in its own task.
async fn boot(
    label: String,
    launcher: Arc<dyn Launch>,
    root: &Path,
    options: Option<Value>,
    timeout: std::time::Duration,
) -> Result<Arc<Client>, LspError> {
    let channel = launcher
        .launch()
        .await
        .map_err(|err| LspError::Launch(label.clone(), err.to_string()))?;
    let client = Client::start(launcher.label(), channel);
    let root_uri = uri::to_uri(root).map_err(|err| LspError::Invalid(err.to_string()))?;
    match client.handshake(root, &root_uri, options, timeout).await {
        Ok(_) => {
            tracing::info!(server = %label, "language server finished its handshake");
            Ok(client)
        }
        Err(err) => {
            // A failed handshake leaves the child running; not cleaning up here orphans one server per attempt.
            client.shutdown().await;
            Err(err)
        }
    }
}

#[async_trait]
impl LanguageServers for StdioServers {
    fn languages(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.id.clone()).collect()
    }

    async fn ask(&self, query: &Query) -> Result<Answer, LspError> {
        let path = self.resolve(&query.path)?;
        let entry = self.entry_for(&path).ok_or_else(|| {
            LspError::NoServer(
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| format!("tệp `.{ext}`"))
                    .unwrap_or_else(|| path.display().to_string()),
            )
        })?;

        let text = tokio::fs::read_to_string(&path).await.map_err(|err| {
            LspError::Invalid(format!("không đọc được {}: {err}", path.display()))
        })?;
        let file_uri = uri::to_uri(&path).map_err(|err| LspError::Invalid(err.to_string()))?;

        let client = self.ready_client(entry).await?;
        let stamp = client.diagnostics_stamp(&file_uri);
        client
            .sync_document(&file_uri, language_id(&path), &text)
            .await?;

        if query.op == Operation::Diagnostics {
            let notes = match client
                .wait_diagnostics(&file_uri, stamp, self.limits.diagnostics)
                .await
            {
                Some(notes) => notes,
                // Timed out with no new publish: if the server ever published for this file, the old copy is the best answer we have.
                None => client.diagnostics(&file_uri).unwrap_or_default(),
            };
            let notes = notes.iter().map(|note| to_note(&text, note)).collect();
            return Ok(Answer::Diagnostics {
                notes,
                busy: client.busy(),
            });
        }

        let (line, character) = to_lsp_position(&text, query.line, query.column)?;
        let position = json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": character },
        });

        let (method, params) = match query.op {
            Operation::Definition => ("textDocument/definition", position),
            Operation::Hover => ("textDocument/hover", position),
            Operation::References => {
                let mut params = position;
                if let Some(object) = params.as_object_mut() {
                    // Always include the declaration; a reference list missing it forces the model to ask again.
                    object.insert("context".into(), json!({ "includeDeclaration": true }));
                }
                ("textDocument/references", params)
            }
            Operation::Diagnostics => unreachable!("đã xử lý ở nhánh trên"),
        };

        let result = client.request(method, params, self.limits.request).await?;
        let busy = client.busy();

        if query.op == Operation::Hover {
            return Ok(Answer::Hover {
                text: hover_text(&result),
                busy,
            });
        }

        let raw = locations(&result);
        let truncated = raw.len() > self.limits.max_locations;
        let mut hits = Vec::new();
        let mut cache: HashMap<PathBuf, Option<String>> = HashMap::new();
        for (target, line, character) in raw.into_iter().take(self.limits.max_locations) {
            match self.hit(&mut cache, &target, line, character).await {
                Some(hit) => hits.push(hit),
                None => {
                    tracing::debug!(uri = %target, "dropping a location that cannot be mapped to a path")
                }
            }
        }
        Ok(Answer::Locations {
            hits,
            truncated,
            busy,
        })
    }
}

impl StdioServers {
    /// One LSP location turned into a line the model can read.
    async fn hit(
        &self,
        cache: &mut HashMap<PathBuf, Option<String>>,
        target: &str,
        line: u32,
        character: u32,
    ) -> Option<Hit> {
        let path = uri::from_uri(target).ok()?;
        // The same question on the way out as on the way in; `resolve_read` also decides `reachable`, so the two answers cannot diverge.
        let inside = self.roots.resolve_read(&path).ok();

        let text = match &inside {
            Some(resolved) => {
                if !cache.contains_key(resolved) {
                    let loaded = tokio::fs::read_to_string(resolved).await.ok();
                    cache.insert(resolved.clone(), loaded);
                }
                cache.get(resolved).and_then(Option::as_ref)
            }
            None => None,
        };

        let (line, column, snippet) = match text {
            Some(text) => from_lsp_position(text, line, character),
            // Outside the working directory means no read and no source line: `pai-fs` boundaries have no exception for this crate.
            None => (line + 1, character + 1, String::new()),
        };

        // Shorten to a relative path when inside the working directory: that is the shape `read` and `grep` accept.
        let display = inside
            .as_ref()
            .and_then(|resolved| resolved.strip_prefix(&self.workspace).ok())
            .map(|relative| relative.display().to_string())
            .unwrap_or_else(|| path.display().to_string());

        Some(Hit {
            path: display,
            line,
            column,
            text: snippet,
            reachable: inside.is_some(),
        })
    }
}

fn to_note(text: &str, note: &RawNote) -> Note {
    let (line, column, _) = from_lsp_position(text, note.line, note.character);
    Note {
        line,
        column,
        severity: match note.severity {
            1 => "lỗi",
            2 => "cảnh báo",
            3 => "thông tin",
            _ => "gợi ý",
        },
        source: note.source.clone(),
        message: note.message.clone(),
    }
}

/// `Location | Location[] | LocationLink[] | null` -> a list of `(uri, line, column)`; all three shapes come from the spec, so accepting all three keeps answers server-independent.
fn locations(result: &Value) -> Vec<(String, u32, u32)> {
    fn one(item: &Value, out: &mut Vec<(String, u32, u32)>) {
        // `LocationLink` uses `targetUri`; `targetSelectionRange` points at the name while `targetRange` spans the body, and the name is what the model wants.
        if let Some(uri) = item.get("targetUri").and_then(Value::as_str) {
            let range = item
                .get("targetSelectionRange")
                .or_else(|| item.get("targetRange"));
            if let Some(range) = range {
                out.push((uri.to_string(), start_line(range), start_character(range)));
            }
            return;
        }
        if let Some(uri) = item.get("uri").and_then(Value::as_str)
            && let Some(range) = item.get("range")
        {
            out.push((uri.to_string(), start_line(range), start_character(range)));
        }
    }

    let mut out = Vec::new();
    match result {
        Value::Array(items) => {
            for item in items {
                one(item, &mut out);
            }
        }
        Value::Object(_) => one(result, &mut out),
        _ => {}
    }
    out
}

fn start_line(range: &Value) -> u32 {
    range
        .pointer("/start/line")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn start_character(range: &Value) -> u32 {
    range
        .pointer("/start/character")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

/// `Hover.contents`: a string, a `MarkedString`, an array of those, or `MarkupContent`.
fn hover_text(result: &Value) -> String {
    fn piece(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) => out.push(text.clone()),
            Value::Array(items) => {
                for item in items {
                    piece(item, out);
                }
            }
            Value::Object(map) => {
                // `MarkupContent {kind, value}` and `MarkedString {language, value}` differ only in the label; `value` is what we need.
                if let Some(text) = map.get("value").and_then(Value::as_str) {
                    out.push(text.to_string());
                }
            }
            _ => {}
        }
    }
    let mut parts = Vec::new();
    piece(result.get("contents").unwrap_or(&Value::Null), &mut parts);
    parts.join("\n").trim().to_string()
}

/// (1-based line, 1-based character column) -> (0-based line, UTF-16 code unit column); an over-long column clamps to end of line, but an out-of-range line is an error rather than a guess.
fn to_lsp_position(text: &str, line: u32, column: u32) -> Result<(u32, u32), LspError> {
    if line == 0 || column == 0 {
        return Err(LspError::Invalid(
            "`line` và `character` đếm từ 1, không từ 0".into(),
        ));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let index = (line - 1) as usize;
    let Some(row) = lines.get(index) else {
        return Err(LspError::Invalid(format!(
            "tệp chỉ có {} dòng nên không có dòng {line}",
            lines.len()
        )));
    };
    let row = row.strip_suffix('\r').unwrap_or(row);
    let utf16: u32 = row
        .chars()
        .take((column - 1) as usize)
        .map(|c| c.len_utf16() as u32)
        .sum();
    Ok((line - 1, utf16))
}

/// The reverse, plus the source line itself.
fn from_lsp_position(text: &str, line: u32, character: u32) -> (u32, u32, String) {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(row) = lines.get(line as usize) else {
        return (line + 1, character + 1, String::new());
    };
    let row = row.strip_suffix('\r').unwrap_or(row);

    let mut units = 0u32;
    let mut chars = 0u32;
    for ch in row.chars() {
        if units >= character {
            break;
        }
        units += ch.len_utf16() as u32;
        chars += 1;
    }
    (line + 1, chars + 1, row.trim().to_string())
}
