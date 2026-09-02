//! `bash` — run a shell command.
//!
//! The most dangerous tool in the set, so two decisions are written down here rather than
//! left implicit:
//!
//! **Asking is the default.** `meta().mutating` is set, and the plugin attaches a guard
//! that routes every call through `PreDecision::Ask`. This is the default, not an option:
//! a tool that runs commands with no path to ask the user has no safe mode to fall back
//! to.
//!
//! **There is no blocklist.** No filtering of `rm -rf`, none of `curl | sh`. A blocklist
//! for shell commands always leaks — `r''m`, `$(echo cm0K | base64 -d)`, an intermediate
//! script — and what it produces is not safety but the feeling of safety, the thing that
//! gets people to click "allow" without reading. The real defences are approval and the
//! sandbox.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::jobs::{JobState, Jobs};
use crate::provider::{Request, ShellExecutor};

/// Without a deadline, a command waiting on input hangs the whole turn until the user
/// gives up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BashArgs {
    /// The command to run, interpreted by `/bin/sh -c`.
    pub command: String,
    /// Deadline in milliseconds. Defaults to 120000.
    pub timeout: Option<u64>,
    /// Run in the background and return a `job_id` immediately. For long-lived processes.
    #[serde(default)]
    pub run_in_background: bool,
}

pub struct Bash {
    shell: Arc<dyn ShellExecutor>,
    jobs: Arc<Jobs>,
    cwd: PathBuf,
    overflow: Overflow,
}

impl Bash {
    pub const NAME: &'static str = "bash";

    pub fn new(
        shell: Arc<dyn ShellExecutor>,
        jobs: Arc<Jobs>,
        cwd: PathBuf,
        overflow: Overflow,
    ) -> Bash {
        Bash {
            shell,
            jobs,
            cwd,
            overflow,
        }
    }
}

#[async_trait]
impl Tool for Bash {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Bash::NAME,
            "Chạy một lệnh shell trong thư mục làm việc. Với tiến trình sống lâu (máy chủ \
             phát triển, theo dõi tệp), đặt `run_in_background` rồi lấy kết quả bằng \
             `job_output`. Output quá dài được gấp lại thành phần đầu và phần cuối; toàn \
             văn vẫn lấy lại được.",
            json_schema_for::<BashArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Not marked `concurrency_safe`: two commands running in parallel in the same
        // working directory can step on each other, and the shell tells nobody.
        ToolMeta::mutating().untrusted().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: BashArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let cwd = self.cwd.clone();
        let shown_cwd = cwd.display().to_string();
        let timeout = Some(
            args.timeout
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_TIMEOUT),
        );

        if args.run_in_background {
            // Its own token, not the turn's: the job has to outlive the turn that
            // started it, or `run_in_background` is no different from running normally.
            let cancel = CancellationToken::new();
            let job = self
                .jobs
                .start(args.command.clone(), shown_cwd.clone(), cancel.clone());
            let shell = self.shell.clone();
            let request = Request {
                command: args.command.clone(),
                cwd,
                timeout: None,
                cancel,
            };
            let tracked = job.clone();
            tokio::spawn(async move {
                let outcome = shell.run(request).await.unwrap_or_default();
                *tracked.state.lock() = JobState::Finished(outcome);
            });

            let meta = json!({
                "command": args.command,
                "cwd": shown_cwd,
                "output": "",
                "exit_code": serde_json::Value::Null,
                "background": true,
                "job_id": job.id,
            });
            return Ok(ToolOutcome::ok(format!(
                "Đã chạy nền, job `{}`. Dùng `job_output` để lấy kết quả.",
                job.id
            ))
            .with_meta("terminal", meta));
        }

        let request = Request {
            command: args.command.clone(),
            cwd,
            timeout,
            cancel: call.cancel_token(),
        };
        let run = self
            .shell
            .run(request)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let mut rendered = run.output.clone();
        if let Some(reason) = &run.interrupted {
            rendered.push_str(&format!("\n[{reason}]\n"));
        }
        match run.exit_code {
            Some(0) | None => {}
            Some(code) => rendered.push_str(&format!("\n[mã thoát {code}]\n")),
        }
        if rendered.trim().is_empty() {
            rendered = "(không có output)".to_string();
        }

        // Command output is large output too: `cargo build` on a medium repo emits a few
        // hundred KiB, and the tail — where the exit code and the last error line are — is
        // the most valuable part. So keep both ends, not just the head.
        let folded = self.overflow.fold(&call.name, rendered, |_| {
            "Chạy lại và lọc ngay trong lệnh (`| tail -n 200`, `| grep …`) nếu bạn cần \
             phần giữa."
                .to_string()
        });

        let meta = json!({
            "command": args.command,
            "cwd": shown_cwd,
            "output": run.output,
            "exit_code": run.exit_code,
            "signal": run.signal,
            "background": false,
            "job_id": serde_json::Value::Null,
        });

        // A non-zero exit code is **not** `is_error`: the command did exactly what it was
        // told, and a red test suite is a useful result, not a failed tool call.
        let mut outcome = ToolOutcome::ok(folded.content).with_meta("terminal", meta);
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
        Ok(outcome)
    }
}
