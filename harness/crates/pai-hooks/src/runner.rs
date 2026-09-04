//! Run one hook command and read its decision.

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// A slow hook makes the tool call slow, and the user has no idea why. Cut it short.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// What the hook reads on stdin.
#[derive(Debug, Clone, Serialize)]
pub struct HookInput<'a> {
    /// `pre-execute` or `post-execute`.
    pub event: &'a str,
    /// The tool name in dotted form: the canonical form, not the wire form.
    pub tool: &'a str,
    pub call_id: &'a str,
    pub arguments: &'a Map<String, Value>,
    /// Only present on `post-execute`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<&'a str>,
}

/// What the hook writes to stdout; there is no "ask" variant, since asking is `Approver`'s job.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum HookDecision {
    Allow,
    Deny {
        /// Goes straight to the model, so it has to tell the model what to do instead.
        reason: String,
    },
}

/// Run a hook; `None` means it could not answer, which the fail-open rule treats as allow.
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
        .inspect_err(|err| tracing::warn!(command, "could not run hook: {err}"))
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        // A write error here usually means the hook simply does not read stdin; carry on.
        let _ = stdin.write_all(&payload).await;
        let _ = stdin.shutdown().await;
    }

    let output = match tokio::time::timeout(deadline, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            tracing::warn!(command, "hook failed: {err}");
            return None;
        }
        Err(_) => {
            tracing::warn!(command, ?deadline, "hook timed out");
            return None;
        }
    };

    if !output.status.success() {
        tracing::warn!(command, code = output.status.code(), "hook exited non-zero");
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<HookDecision>(text.trim()) {
        Ok(decision) => Some(decision),
        Err(err) => {
            tracing::warn!(command, "hook returned something unreadable: {err}");
            None
        }
    }
}
