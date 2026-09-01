//! Một kết nối tới một language server: bắt tay, hỏi–đáp, và chết cho tử tế.
//!
//! Bốn bất biến, và mỗi cái đều là một cách hỏng đã thấy trước:
//!
//! **1. Không truy vấn nào đi trước cái bắt tay.** [`Client::request`] từ chối mọi phương
//! thức khi cờ `ready` chưa bật, và cờ đó chỉ bật sau khi `initialize` có trả lời *và*
//! `initialized` đã gửi đi. Một `textDocument/definition` gửi sớm không được server trả
//! lời tử tế — nó bị coi là lỗi giao thức, và tuỳ server mà kết nối bị đóng hoặc câu hỏi
//! bị nuốt. Cái giá của việc kiểm là một `AtomicBool`; cái giá của việc không kiểm là một
//! lỗi chỉ hiện ra trên máy của người dùng nào có server khởi động chậm hơn ta.
//!
//! **2. Server chết là mọi câu hỏi đang treo được trả lời ngay.** Task đọc, khi ống đóng,
//! **rót lỗi vào mọi `oneshot` đang chờ** trước khi thoát. Không làm vậy thì mỗi câu hỏi
//! dở dang phải chờ hết hạn của chính nó — sáu mươi giây nhìn vào một tiến trình đã không
//! còn tồn tại.
//!
//! **3. Yêu cầu từ server luôn được trả lời.** Kể cả yêu cầu ta không hiểu, và lúc đó câu
//! trả lời là một lỗi `MethodNotFound` đúng chuẩn. Một `id` không bao giờ được hồi đáp là
//! một server ngồi chờ vô hạn, và với `rust-analyzer` thì đó là cả việc nạp workspace
//! dừng lại.
//!
//! **4. "Đang bận" là một sự thật được ghi lại, không phải một phỏng đoán.** `$/progress`
//! là cơ chế chuẩn của LSP 3.15; ta đếm token đang mở và đó là toàn bộ cơ sở để nói câu
//! "server còn đang lập chỉ mục". Không có chỗ nào trong crate này đoán theo tên server.

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

/// Một chẩn đoán đúng như server gửi: 0-based, cột tính bằng đơn vị mã UTF-16.
///
/// Giữ nguyên toạ độ của giao thức tới tận chỗ có tệp trong tay để đổi. Đổi sớm, ngay
/// trong task đọc, thì phải đi đọc đĩa từ trong đó — và một lần đọc đĩa chậm ở đấy làm
/// nghẽn mọi tin đang tới trên cùng một ống.
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

/// Thứ mà cả task đọc lẫn [`Client`] cùng nhìn.
struct State {
    label: String,
    alive: AtomicBool,
    /// Vì sao chết. Đọc được trong thông báo lỗi, nên nó phải là một câu chứ không phải
    /// một mã số.
    cause: Mutex<String>,
    /// `uri` → (số thứ tự lần đăng, chẩn đoán). Số thứ tự để phân biệt "server chưa nói
    /// gì" với "server nói rằng không có lỗi nào" — hai chuyện khác hẳn nhau.
    diagnostics: Mutex<HashMap<String, (u64, Vec<RawNote>)>>,
    stamp: AtomicI64,
    fresh: Notify,
    /// Token `$/progress` đang mở.
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
        // Đánh thức cả người đang chờ chẩn đoán: họ chờ một thứ sẽ không bao giờ tới nữa.
        self.fresh.notify_waiters();
    }
}

/// Một tài liệu ta đã mở với server.
struct Doc {
    version: i64,
    /// Vân tay nội dung lần gửi gần nhất. Tệp trên đĩa đổi giữa hai câu hỏi — mô hình vừa
    /// `edit` xong rồi hỏi lỗi biên dịch là đường đi thường nhất — và một server còn giữ
    /// bản cũ trong bộ nhớ sẽ trả lời về mã không còn tồn tại. So vân tay rẻ hơn gửi lại
    /// cả tệp mỗi lần, và đúng hơn nhiều so với không gửi gì.
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
    /// Dựng client và bắt đầu đọc. **Chưa** bắt tay — xem [`Client::handshake`].
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

    /// Server còn đang làm một việc dài (nạp workspace, lập chỉ mục).
    pub fn busy(&self) -> bool {
        !self.state.busy.lock().is_empty()
    }

    fn dead(&self) -> LspError {
        LspError::Dead(self.label.clone(), self.state.cause.lock().clone())
    }

    /// `initialize` → `initialized`. Chỉ sau hàm này client mới nhận truy vấn.
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
            // `rootPath` đã bị spec bỏ từ lâu nhưng vài server vẫn chỉ đọc nó. Gửi cả hai
            // rẻ hơn nhiều so với việc dò xem server nào thuộc nhóm nào.
            "rootPath": root.display().to_string(),
            "workspaceFolders": [{
                "uri": root_uri,
                "name": root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".into()),
            }],
            "capabilities": {
                "workspace": {
                    // Khai `true` rồi trả lời `null` là hợp lệ và có nghĩa "dùng mặc định
                    // của anh". Khai `false` thì vài server dừng luôn phần cấu hình động
                    // và chạy ở một chế độ nghèo hơn mà không nói gì.
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
                // Đây là thứ mở đường cho `$/progress`, và `$/progress` là toàn bộ cơ sở
                // để ta nói được "server còn đang lập chỉ mục" thay vì "không tìm thấy gì".
                "window": { "workDoneProgress": true },
            },
            "initializationOptions": options,
        });

        let result = self.send_request("initialize", params, timeout).await?;
        self.send_notify("initialized", json!({})).await?;
        self.ready.store(true, Ordering::SeqCst);
        Ok(result)
    }

    /// Một truy vấn. Từ chối nếu chưa bắt tay xong — xem bất biến 1 ở đầu tệp.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        if !self.ready.load(Ordering::SeqCst) {
            // Lỗi của **ta**, không của server: một chỗ trong harness đã hỏi trước khi bắt
            // tay xong. Nói ra như vậy thay vì mượn câu "server chưa sẵn sàng", vì hai
            // chuyện đó được sửa ở hai nơi khác nhau.
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
            // Đầu gửi bị thả mà không gửi gì: chỉ xảy ra khi task đọc thoát giữa chừng.
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
                // Ghi hỏng nghĩa là đầu kia đã đóng. Tuyên bố chết ngay tại đây chứ không
                // chờ task đọc phát hiện ra: người gọi đang cầm lỗi này trong tay, và câu
                // hỏi tiếp theo không nên được gửi vào một cái ống đã đứt.
                self.state.die(err.to_string(), &self.pending);
                Err(self.dead())
            }
        }
    }

    /// Bảo đảm server đang giữ đúng nội dung của tệp này.
    pub async fn sync_document(
        &self,
        uri: &str,
        language_id: &str,
        text: &str,
    ) -> Result<(), LspError> {
        let digest = fingerprint(text);
        // Khoá bất đồng bộ và giữ qua cả lần gửi: hai truy vấn chồng nhau trên cùng một
        // tệp mà cùng thấy "chưa mở" sẽ gửi `didOpen` hai lần, và một server nhận hai
        // `didOpen` cho một URI là một server có hai bản của cùng một tài liệu.
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
                        // Thay cả tệp, không gửi delta: ta không theo dõi từng phím gõ,
                        // ta chỉ biết tệp trên đĩa đã khác. Dựng một delta từ hai bản đầy
                        // đủ là làm thêm việc để gửi đi ít byte hơn trên một ống nội bộ.
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

    /// Số thứ tự lần đăng chẩn đoán gần nhất cho `uri`. Không có thì `0`.
    pub fn diagnostics_stamp(&self, uri: &str) -> u64 {
        self.state
            .diagnostics
            .lock()
            .get(uri)
            .map(|(stamp, _)| *stamp)
            .unwrap_or(0)
    }

    /// Lần đăng gần nhất, không chờ. `None` là server chưa từng nói gì về tệp này.
    pub fn diagnostics(&self, uri: &str) -> Option<Vec<RawNote>> {
        self.state
            .diagnostics
            .lock()
            .get(uri)
            .map(|(_, notes)| notes.clone())
    }

    /// Chờ một lần đăng **mới hơn** `since`. `None` là hết giờ mà server chưa nói gì.
    pub async fn wait_diagnostics(
        &self,
        uri: &str,
        since: u64,
        timeout: Duration,
    ) -> Option<Vec<RawNote>> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            // Đăng ký chờ **trước** khi xem, nếu không một lần đăng chen vào giữa hai
            // bước sẽ mất và ta ngồi chờ hết giờ với câu trả lời đã nằm sẵn trong tay.
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

    /// `shutdown` → `exit` → giết nếu cần.
    ///
    /// Ba bước chứ không một, vì mỗi bước bắt một loại server: cái ngoan thoát ở bước hai,
    /// cái đang kẹt trong một vòng lặp dài thì không, và cái đã chết rồi thì bước một
    /// hỏng ngay và ta đi thẳng xuống bước ba.
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
                tracing::warn!(server = %self.label, "language server không thoát sau `exit`; giết nó");
                let _ = child.kill().await;
            }
        }
        self.state.die("đã đóng theo yêu cầu".into(), &self.pending);
    }
}

/// FNV-1a. Không cần chống va chạm có chủ ý — nó chỉ trả lời "tệp có đổi không" giữa hai
/// lần hỏi của cùng một phiên, và một dependency mật mã cho việc đó là trả tiền cho thứ
/// không dùng. Cùng lý lẽ với `pai-index::plugin::db_name`.
fn fingerprint(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Task đọc: một tin một vòng, cho tới khi ống đóng.
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
        // Trả lời cho câu ta hỏi.
        (Some(id), None) => {
            let Some(tx) = pending.lock().remove(&id) else {
                tracing::debug!(server = %state.label, id, "trả lời cho một câu hỏi đã bỏ");
                return;
            };
            let outcome = match message.get("error") {
                Some(error) => Err(LspError::Protocol(describe(error))),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(outcome);
        }
        // Server hỏi ngược lại. Luôn phải trả lời — xem bất biến 3.
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
        (None, None) => tracing::debug!(server = %state.label, "tin không có `id` lẫn `method`"),
    }
}

/// Trả lời một yêu cầu của server. `None` = ta không cài phương thức đó.
fn answer(method: &str, message: &Value, state: &Arc<State>) -> Option<Value> {
    match method {
        // Server xin một token tiến trình. Ghi nhận ngay ở đây chứ không đợi `$/progress`
        // với `kind: "begin"`: giữa hai tin đó là đúng khoảng thời gian ta hay bị hỏi, và
        // trả lời "rảnh" trong khoảng đó là nói sai về một việc đang bắt đầu.
        "window/workDoneProgress/create" => {
            if let Some(token) = message.pointer("/params/token") {
                state.busy.lock().insert(token_key(token));
            }
            Some(Value::Null)
        }
        // `null` cho mỗi mục nghĩa là "không có cấu hình riêng, dùng mặc định". Đó là câu
        // trả lời đúng: harness không có tệp cấu hình cho từng language server, và bịa ra
        // một cái ở đây là làm thay người dùng một lựa chọn họ chưa nói gì về nó.
        "workspace/configuration" => {
            let count = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            Some(Value::Array(vec![Value::Null; count]))
        }
        "workspace/workspaceFolders" => Some(Value::Null),
        // Ta không có sổ đăng ký khả năng động; nhận rồi bỏ qua là đúng, vì mọi phương
        // thức ta gọi đều là phương thức tĩnh của spec.
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
        _ => tracing::trace!(server = %state.label, method, "thông báo bỏ qua"),
    }
}

/// Token tiến trình là `string | integer`. Quy về chuỗi để một `HashSet` là đủ.
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
        // Spec cho phép vắng `severity`, và lúc đó "client tự quyết". Coi nó là lỗi là
        // chiều sai an toàn: một cảnh báo bị báo thành lỗi làm mô hình đi xem, một lỗi bị
        // báo thành gợi ý làm nó bỏ qua.
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
