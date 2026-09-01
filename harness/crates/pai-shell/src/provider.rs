//! Seam thi hành lệnh, và bản cục bộ của nó.
//!
//! Ý duy nhất đáng nhớ trong tệp này: **cái ta chạy là một cây tiến trình, không phải một
//! tiến trình.** `sh -c "npm test"` sinh ra `npm`, sinh ra `node`. Giết cái shell để lại
//! cả hai đứa kia, và chúng giữ cổng, giữ khoá tệp, ghi tiếp vào cùng thư mục — sau một
//! lượt bị huỷ thì lượt sau chạy trong một cái máy đã bị nhiễm.
//!
//! Nên tiến trình con được đặt vào **nhóm tiến trình riêng**, và mọi tín hiệu gửi cho cả
//! nhóm. Đây là chỗ dễ làm sai nhất trong crate này, và cũng là chỗ sai không bao giờ tự
//! lộ ra: mọi thứ trông vẫn chạy.

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
    /// Bị cắt vì hết giờ hoặc vì bị huỷ. Kết quả một phần vẫn có ích, nhưng phải nói rõ.
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

/// Gửi tín hiệu cho cả nhóm tiến trình.
#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    // Số âm nghĩa là "cả nhóm". Đây là toàn bộ lý do phải đặt process group ở trên.
    unsafe { libc::kill(-(pid as i32), signal) };
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {
    // Windows không có nhóm tiến trình theo nghĩa này; việc tương ứng là Job Object và
    // nó thuộc về `pai-sandbox`. Cho tới lúc đó, chỉ tiến trình trực tiếp bị giết.
}

/// Đĩa của chính máy này.
///
/// Nó giữ một `Context` để hỏi seam giam tiến trình **tại thời điểm spawn**, không phải
/// lúc dựng: gỡ plugin sandbox ra phải làm mọi lệnh sau đó chạy không giam, chứ không
/// phải đi qua một bản sao còn sót lại của provider cũ.
pub struct LocalShell {
    ctx: Context,
    policy: Policy,
}

impl LocalShell {
    pub fn new(ctx: Context, policy: Policy) -> LocalShell {
        LocalShell { ctx, policy }
    }

    /// argv sau khi đã bọc, nếu có ai bọc.
    ///
    /// Không có provider thì chạy argv gốc — đó là hành vi đúng khi chưa ai cắm vòng
    /// giam. Nhưng provider **có** mà bọc **hỏng** thì không chạy: một lần bỏ qua im
    /// lặng là một lần người dùng tin vào một vòng vây không tồn tại.
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

        // stdout và stderr gộp theo thứ tự tới, không tách hai khối: một dòng lỗi in ra
        // giữa chừng chỉ có nghĩa khi biết nó nằm giữa những dòng nào.
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

        // Giết cả nhóm, không chỉ cái shell.
        if let Some(pid) = pid {
            signal_group(pid, libc_sigterm());
            // Cho cây tiến trình một khoảnh khắc để tự dọn, rồi mới ép.
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

/// `None` nghĩa là không có hạn giờ: chờ mãi thay vì chờ 0 giây.
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
