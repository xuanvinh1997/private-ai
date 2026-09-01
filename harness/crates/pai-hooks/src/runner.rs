//! Chạy một lệnh hook và đọc quyết định của nó.

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Hook chậm thì lời gọi tool chậm theo, và người dùng không biết vì sao. Cắt sớm.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Cái hook đọc trên stdin.
#[derive(Debug, Clone, Serialize)]
pub struct HookInput<'a> {
    /// `pre-execute` hoặc `post-execute`.
    pub event: &'a str,
    /// Tên tool ở dạng có dấu chấm — dạng chuẩn, không phải dạng trên dây.
    pub tool: &'a str,
    pub call_id: &'a str,
    pub arguments: &'a Map<String, Value>,
    /// Chỉ có ở `post-execute`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<&'a str>,
}

/// Cái hook ghi ra stdout.
///
/// Không có biến thể "hỏi": một hook là một lệnh chạy không có người ngồi đó, nên nó
/// không có cách nào hỏi ai. Muốn hỏi thì đó là việc của `Approver`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum HookDecision {
    Allow,
    Deny {
        /// Đi thẳng tới mô hình, nên nó phải nói được cho mô hình biết nên làm gì khác.
        reason: String,
    },
}

/// Chạy một hook. `None` nghĩa là hook không trả lời được — xem luật fail-open ở đầu crate.
pub async fn run(command: &str, input: &HookInput<'_>, deadline: Duration) -> Option<HookDecision> {
    let payload = serde_json::to_vec(input).ok()?;

    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .inspect_err(|err| tracing::warn!(command, "không chạy được hook: {err}"))
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        // Lỗi ghi ở đây gần như luôn là hook đã đóng stdin vì nó không đọc — không phải
        // lý do để dừng, cứ chờ nó nói gì.
        let _ = stdin.write_all(&payload).await;
        let _ = stdin.shutdown().await;
    }

    let output = match tokio::time::timeout(deadline, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            tracing::warn!(command, "hook hỏng: {err}");
            return None;
        }
        Err(_) => {
            tracing::warn!(command, ?deadline, "hook hết giờ");
            return None;
        }
    };

    if !output.status.success() {
        tracing::warn!(
            command,
            code = output.status.code(),
            "hook thoát với mã khác 0"
        );
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<HookDecision>(text.trim()) {
        Ok(decision) => Some(decision),
        Err(err) => {
            tracing::warn!(command, "hook trả về thứ không đọc được: {err}");
            None
        }
    }
}
