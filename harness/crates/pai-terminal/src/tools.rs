//! Six terminal tools, names and schemas taken verbatim from dsh -- including the camel-cased
//! `sessionId` -- because the model learned this tool set from public data. No `terminal_resize`:
//! window size belongs to the UI showing the session, so resizing goes through the seam instead.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::seam::{OpenRequest, Owner, Sent, Signal, Stop, TerminalError, TerminalHost, Wait};

/// How long without new output counts as the command having settled.
const DEFAULT_QUIET: Duration = Duration::from_millis(300);
/// Cap for a foreground send; past it we return what we have plus an explicit note.
const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Lines per read, matching dsh's default.
const DEFAULT_COUNT: usize = 500;
/// Hard cap per read, so a huge `count` cannot dump the whole buffer into context in one call.
const MAX_COUNT: usize = 2_000;

fn invalid(err: impl std::fmt::Display) -> ToolError {
    ToolError::Invalid(err.to_string())
}

fn args<T: serde::de::DeserializeOwned>(call: &Invocation) -> Result<T, ToolError> {
    serde_json::from_value(Value::Object(call.arguments.clone())).map_err(invalid)
}

/// Seam errors reach the model as tool errors, not as successful text: a missing session means the call was wrong.
fn failed(err: TerminalError) -> ToolError {
    match err {
        TerminalError::NoSession(_) | TerminalError::NoBackend(_, _) => {
            ToolError::Invalid(err.to_string())
        }
        other => ToolError::Failed(other.to_string()),
    }
}

/// The text block the model reads, plus an explicit note about lines dropped from the buffer.
fn render(lines: &[String], dropped: usize, max_lines: usize) -> String {
    let mut text = if lines.is_empty() {
        "(không có output)".to_string()
    } else {
        lines.join("\n")
    };
    if dropped > 0 {
        text.push_str(&format!(
            "\n\n[bộ đệm chỉ giữ {max_lines} dòng mới nhất; {dropped} dòng cũ hơn đã bị bỏ]"
        ));
    }
    text
}

/// `meta.terminal` in the shape the UI reads (`ui/src/lib/protocol.ts`); a live session has no exit code, hence `null`.
fn terminal_meta(command: &str, cwd: &str, output: &str, background: bool, id: &str) -> Value {
    json!({
        "command": command,
        "cwd": cwd,
        "output": output,
        "exit_code": Value::Null,
        "signal": Value::Null,
        "background": background,
        "job_id": id,
    })
}

// --- terminal_open ----------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenArgs {
    /// Registered terminal backend type, usually "shell".
    #[serde(rename = "type")]
    pub backend: String,
    /// Optional owner-local display name such as "main" or "gdb".
    pub name: Option<String>,
    /// Initial working directory. Defaults to the deployment workspace root.
    pub cwd: Option<String>,
}

pub struct TerminalOpen {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
}

impl TerminalOpen {
    pub const NAME: &'static str = "terminal_open";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner) -> TerminalOpen {
        TerminalOpen { host, owner }
    }
}

#[async_trait]
impl Tool for TerminalOpen {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalOpen::NAME,
            "Tạo một phiên terminal bền, chỉ thuộc về agent này, từ một backend đã đăng ký. \
             Dùng khi trạng thái phải sống qua nhiều lần gọi: thư mục hiện tại sau `cd`, một \
             REPL đang mở, một máy chủ phát triển vẫn đang chạy. Một lệnh không cần trạng \
             thái thì `bash` rẻ hơn và không để lại gì phải dọn.",
            json_schema_for::<OpenArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Not `concurrency_safe`: two sessions opened in parallel in the same cwd can trample each other silently.
        ToolMeta::mutating().untrusted().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let parsed: OpenArgs = args(call)?;
        let info = self
            .host
            .open(
                self.owner,
                OpenRequest {
                    backend: parsed.backend,
                    name: parsed.name,
                    cwd: parsed.cwd.map(Into::into),
                },
            )
            .await
            .map_err(failed)?;

        let text = format!(
            "Đã mở phiên `{}` ({}) tại {}. Gửi lệnh bằng `terminal_send`, đọc bằng \
             `terminal_read`, và đóng bằng `terminal_close` khi xong.",
            info.id, info.name, info.cwd
        );
        let meta = terminal_meta(
            &format!("terminal_open {}", info.backend),
            &info.cwd,
            "",
            false,
            &info.id,
        );
        Ok(ToolOutcome::ok(text)
            .with_structured(serde_json::to_value(&info).unwrap_or(Value::Null))
            .with_meta("terminal", meta))
    }
}

// --- terminal_list ----------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

pub struct TerminalList {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
}

impl TerminalList {
    pub const NAME: &'static str = "terminal_list";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner) -> TerminalList {
        TerminalList { host, owner }
    }
}

#[async_trait]
impl Tool for TerminalList {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalList::NAME,
            "Liệt kê những phiên terminal bền thuộc về agent này.",
            json_schema_for::<NoArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let rows = self.host.list(self.owner);
        if rows.is_empty() {
            return Ok(ToolOutcome::ok("Không có phiên terminal nào."));
        }
        let text = rows
            .iter()
            .map(|info| {
                let state = if info.alive {
                    "đang chạy"
                } else {
                    "đã kết thúc"
                };
                format!("{}  {}  {}  ({state})", info.id, info.name, info.cwd)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolOutcome::ok(text)
            .with_structured(serde_json::to_value(&rows).unwrap_or(Value::Null)))
    }
}

// --- terminal_read ----------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Newest-relative line offset (default 0).
    pub offset: Option<usize>,
    /// Requested line count (default 500; backend caps apply).
    pub count: Option<usize>,
}

pub struct TerminalRead {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
    max_lines: usize,
}

impl TerminalRead {
    pub const NAME: &'static str = "terminal_read";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner, max_lines: usize) -> TerminalRead {
        TerminalRead {
            host,
            owner,
            max_lines,
        }
    }
}

#[async_trait]
impl Tool for TerminalRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalRead::NAME,
            "Đọc một trang output đang được giữ của một phiên terminal bền, không gửi gì vào. \
             `offset` đếm ngược từ dòng mới nhất.",
            json_schema_for::<ReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let parsed: ReadArgs = args(call)?;
        let count = parsed.count.unwrap_or(DEFAULT_COUNT).min(MAX_COUNT);
        let page = self
            .host
            .read(
                self.owner,
                &parsed.session_id,
                parsed.offset.unwrap_or(0),
                count,
            )
            .map_err(failed)?;
        let info = self
            .host
            .info(self.owner, &parsed.session_id)
            .map_err(failed)?;

        let text = render(&page.lines, page.dropped, self.max_lines);
        let meta = terminal_meta(
            &format!("terminal_read {}", parsed.session_id),
            &info.cwd,
            &text,
            !info.alive,
            &parsed.session_id,
        );
        Ok(ToolOutcome::ok(text).with_meta("terminal", meta))
    }
}

// --- terminal_send ----------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendArgs {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// UTF-8 text to write to the terminal.
    pub text: String,
    /// Submit Enter after text (default true). Set false for control characters or
    /// incomplete REPL input.
    pub submit: Option<bool>,
    /// Send and return immediately without waiting. Collect output later with `terminal_read`.
    #[serde(default)]
    pub run_in_background: bool,
}

pub struct TerminalSend {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
    max_lines: usize,
}

impl TerminalSend {
    pub const NAME: &'static str = "terminal_send";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner, max_lines: usize) -> TerminalSend {
        TerminalSend {
            host,
            owner,
            max_lines,
        }
    }
}

#[async_trait]
impl Tool for TerminalSend {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalSend::NAME,
            "Gửi văn bản vào một phiên terminal bền. Mặc định có Enter và lời gọi chờ tới khi \
             phiên yên trở lại, hết giờ, hoặc phiên kết thúc. Đặt `run_in_background` cho \
             tiến trình sống lâu rồi lấy output bằng `terminal_read`.",
            json_schema_for::<SendArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::mutating().untrusted().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let parsed: SendArgs = args(call)?;
        let mut bytes = parsed.text.clone().into_bytes();
        if parsed.submit.unwrap_or(true) {
            bytes.push(b'\n');
        }

        let wait = (!parsed.run_in_background).then_some(Wait {
            quiet: DEFAULT_QUIET,
            timeout: DEFAULT_SEND_TIMEOUT,
        });
        let Sent {
            lines,
            dropped,
            stopped,
        } = self
            .host
            .send(self.owner, &parsed.session_id, &bytes, wait)
            .await
            .map_err(failed)?;

        let info = self
            .host
            .info(self.owner, &parsed.session_id)
            .map_err(failed)?;

        let mut text = render(&lines, dropped, self.max_lines);
        // The stop reason ships with the result, because "settled" and "timed out" imply different next steps.
        match stopped {
            Stop::Quiet => {}
            Stop::Background => {
                text = format!(
                    "Đã gửi vào phiên `{}` và không chờ. Dùng `terminal_read` để lấy output.",
                    parsed.session_id
                );
            }
            Stop::Timeout => text.push_str(&format!(
                "\n\n[vẫn đang chạy sau {} giây; phiên còn sống, đọc tiếp bằng `terminal_read`]",
                DEFAULT_SEND_TIMEOUT.as_secs()
            )),
            Stop::Ended => text.push_str("\n\n[phiên đã kết thúc]"),
        }

        let meta = terminal_meta(
            &parsed.text,
            &info.cwd,
            &text,
            parsed.run_in_background,
            &parsed.session_id,
        );
        Ok(ToolOutcome::ok(text).with_meta("terminal", meta))
    }
}

// --- terminal_signal --------------------------------------------------------------------

/// Closed signal set. `inline` rather than a schemars `$ref`, since this schema goes straight to the model
/// and some converter along the way will flatten the reference wrongly or drop it.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[schemars(inline)]
pub enum SignalName {
    #[serde(rename = "SIGINT")]
    Int,
    #[serde(rename = "SIGTERM")]
    Term,
    #[serde(rename = "SIGKILL")]
    Kill,
    #[serde(rename = "SIGTSTP")]
    Tstp,
    #[serde(rename = "SIGHUP")]
    Hup,
}

impl From<SignalName> for Signal {
    fn from(name: SignalName) -> Signal {
        match name {
            SignalName::Int => Signal::Int,
            SignalName::Term => Signal::Term,
            SignalName::Kill => Signal::Kill,
            SignalName::Tstp => Signal::Tstp,
            SignalName::Hup => Signal::Hup,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignalArgs {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// Shell-targeted SIGKILL is rejected; use terminal_close.
    pub signal: SignalName,
}

pub struct TerminalSignal {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
}

impl TerminalSignal {
    pub const NAME: &'static str = "terminal_signal";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner) -> TerminalSignal {
        TerminalSignal { host, owner }
    }
}

#[async_trait]
impl Tool for TerminalSignal {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalSignal::NAME,
            "Gửi một tín hiệu cho nhóm tiến trình tiền cảnh của một phiên terminal bền. \
             `SIGKILL` nhắm vào chính shell của phiên bị từ chối — dùng `terminal_close`.",
            json_schema_for::<SignalArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::mutating().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let parsed: SignalArgs = args(call)?;
        let signal: Signal = parsed.signal.into();
        self.host
            .signal(self.owner, &parsed.session_id, signal)
            .map_err(failed)?;
        Ok(ToolOutcome::ok(format!(
            "Đã gửi {} cho phiên `{}`.",
            signal.as_str(),
            parsed.session_id
        )))
    }
}

// --- terminal_close ---------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseArgs {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

pub struct TerminalClose {
    host: Arc<dyn TerminalHost>,
    owner: Owner,
}

impl TerminalClose {
    pub const NAME: &'static str = "terminal_close";

    pub fn new(host: Arc<dyn TerminalHost>, owner: Owner) -> TerminalClose {
        TerminalClose { host, owner }
    }
}

#[async_trait]
impl Tool for TerminalClose {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TerminalClose::NAME,
            "Đóng một phiên terminal bền và chờ cho tới khi cả cây tiến trình của nó biến mất.",
            json_schema_for::<CloseArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::mutating().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let parsed: CloseArgs = args(call)?;
        self.host
            .close(self.owner, &parsed.session_id)
            .await
            .map_err(failed)?;
        Ok(ToolOutcome::ok(format!(
            "Đã đóng phiên `{}` cùng toàn bộ tiến trình con của nó.",
            parsed.session_id
        )))
    }
}
