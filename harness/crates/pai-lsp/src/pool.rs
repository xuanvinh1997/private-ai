//! Bản cài đặt của seam: một tiến trình con cho mỗi ngôn ngữ, dựng lúc cần.
//!
//! Ba việc gộp ở đây, và chúng gộp vì cả ba đều là **biên giới** giữa từ vựng của harness
//! và từ vựng của LSP:
//!
//! **Vòng đời.** Server được dựng ở lần hỏi đầu tiên, không phải lúc cắm plugin. Một
//! `rust-analyzer` khởi động cùng ứng dụng là hàng trăm MB và một lõi CPU tiêu tốn cho
//! một người dùng có thể không hỏi câu nào cả phiên.
//!
//! **Đường dẫn.** Mọi đường dẫn đi vào đều qua [`FileRoots::resolve_read`] — chuẩn hoá
//! trước, kiểm tra sau, đúng luật của `pai-fs`. Mọi đường dẫn đi ra đều được hỏi lại cùng
//! một câu, và câu trả lời đi kèm kết quả dưới dạng cờ `reachable`: một định nghĩa nằm
//! trong `~/.cargo/registry` là một câu trả lời **đúng** mà `read` không với tới, và giấu
//! nó đi thì mô hình kết luận rằng hàm đó không có định nghĩa.
//!
//! **Toạ độ.** LSP đếm dòng từ 0 và đếm cột bằng đơn vị mã UTF-16; `read`, trình soạn
//! thảo và con người đếm từ 1 và đếm bằng ký tự. Phép đổi nằm gọn trong hai hàm ở cuối
//! tệp và **không chỗ nào khác trong crate được phép đổi lại** — một cột lệch không làm
//! câu trả lời sai hẳn, nó chỉ trỏ vào ký hiệu bên cạnh, và đó là loại sai không ai bắt
//! được khi đọc kết quả.

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

/// Một hàng cấu hình đã dò được lệnh và sẵn sàng chạy.
pub struct Entry {
    pub id: String,
    pub extensions: Vec<String>,
    pub launcher: Arc<dyn Launch>,
    pub options: Option<Value>,
}

/// Trạng thái một lần khởi động.
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
            // Chuẩn hoá một lần ở đây: đường dẫn từ server về đã đi qua `canonicalize`,
            // nên nếu gốc chưa chuẩn hoá thì phép `strip_prefix` để rút gọn hiển thị lặng
            // lẽ trượt và mọi kết quả hiện ra dưới dạng tuyệt đối.
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

    /// Chuẩn hoá trước, kiểm tra sau. Đường dẫn tương đối tính từ thư mục làm việc.
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

    /// Lấy một client đã bắt tay xong, chờ **có hạn**.
    ///
    /// Hết hạn không phải là hỏng: task khởi động vẫn chạy tiếp trong nền, nên câu hỏi sau
    /// vài giây nữa thường trúng một server đã sẵn sàng. Cái ta không được làm là ngồi
    /// chờ vô hạn (lượt của người dùng đứng im) hay trả về rỗng (một lời nói dối).
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

    /// Ô của một ngôn ngữ, dựng nếu chưa có hoặc nếu cái cũ đã chết.
    ///
    /// Chết **sau khi từng chạy** thì dựng lại; **không dựng nổi** thì không. Hai chuyện
    /// đó khác nhau ở chỗ thử lại có ích hay không: một server bị OOM giữa chừng sẽ chạy
    /// lại được, còn một lệnh sai tham số sẽ hỏng y hệt ở lần thứ một trăm — và thử lại nó
    /// mỗi lần mô hình hỏi là đẻ một tiến trình cho mỗi câu hỏi.
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
        // Task nền được rộng hạn hơn phía trước. Phía trước bỏ cuộc sớm vì nó đang giữ
        // lượt của người dùng; task nền thì không giữ gì cả, và cắt nó đúng lúc phía
        // trước hết kiên nhẫn sẽ biến "hỏi lại sau vài giây" — câu ta vừa nói với mô
        // hình — thành một lời khuyên sai, vì lần hỏi sau lại bắt đầu từ số không.
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

    /// Đóng mọi server. Dành cho lúc gỡ plugin.
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

/// Dựng và bắt tay. Tách khỏi [`StdioServers`] để nó chạy được trong một task rời.
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
            tracing::info!(server = %label, "language server đã bắt tay xong");
            Ok(client)
        }
        Err(err) => {
            // Bắt tay hỏng thì tiến trình con vẫn đang chạy. Không dọn ở đây là để lại một
            // server mồ côi cho mỗi lần thử — và lần thử tiếp theo đẻ thêm một cái nữa.
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
                // Hết giờ mà không có lần đăng mới: nếu server từng đăng cho tệp này thì
                // bản cũ vẫn là câu trả lời tốt nhất ta có, và nó thường vẫn đúng.
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
                    // Luôn kèm chỗ khai báo. Một danh sách tham chiếu thiếu mất định nghĩa
                    // buộc mô hình hỏi thêm một lần nữa để biết thứ nó vừa đếm là gì.
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
                    tracing::debug!(uri = %target, "bỏ một vị trí không chuyển được về đường dẫn")
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
    /// Một vị trí của LSP thành một dòng mô hình đọc được.
    async fn hit(
        &self,
        cache: &mut HashMap<PathBuf, Option<String>>,
        target: &str,
        line: u32,
        character: u32,
    ) -> Option<Hit> {
        let path = uri::from_uri(target).ok()?;
        // Cùng một câu hỏi cho đường ra như cho đường vào. `resolve_read` cũng là thứ
        // quyết định `reachable`, nên hai câu trả lời không thể lệch nhau.
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
            // Ngoài thư mục làm việc thì không đọc, và dòng mã để trống. Ranh giới của
            // `pai-fs` không có ngoại lệ cho crate này, kể cả khi trích một dòng thì tiện.
            None => (line + 1, character + 1, String::new()),
        };

        // Rút gọn về đường dẫn tương đối khi nằm trong thư mục làm việc: đó là hình dạng
        // mà `read` và `grep` nhận, nên mô hình chép thẳng sang bước sau được.
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

/// `Location | Location[] | LocationLink[] | null` → danh sách `(uri, dòng, cột)`.
///
/// Ba hình dạng cho một câu trả lời là chuyện của spec, không phải của server; nhận đủ cả
/// ba là điều kiện để câu trả lời không phụ thuộc vào server nào đang chạy.
fn locations(result: &Value) -> Vec<(String, u32, u32)> {
    fn one(item: &Value, out: &mut Vec<(String, u32, u32)>) {
        // `LocationLink` dùng `targetUri`; `targetSelectionRange` trỏ vào đúng cái tên,
        // còn `targetRange` bao cả thân hàm — cái mô hình muốn đọc là cái tên.
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

/// `Hover.contents`: chuỗi, `MarkedString`, mảng của chúng, hoặc `MarkupContent`.
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
                // `MarkupContent {kind, value}` và `MarkedString {language, value}` chỉ
                // khác nhau ở cái nhãn; phần ta cần là `value` trong cả hai.
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

/// (dòng 1-based, cột 1-based theo ký tự) → (dòng 0-based, cột theo đơn vị mã UTF-16).
///
/// Cột vượt quá độ dài dòng thì **kẹp về cuối dòng** chứ không báo lỗi: con trỏ ở cuối
/// dòng là một chỗ hợp lệ để hỏi, và một mô hình đếm cột hơi rộng tay vẫn đang hỏi đúng
/// dòng. Dòng vượt quá số dòng của tệp thì ngược lại — đó là một câu hỏi về chỗ không tồn
/// tại, và trả lời nó bằng dòng cuối là bịa.
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

/// Chiều ngược lại, cộng chính dòng mã ở đó.
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
