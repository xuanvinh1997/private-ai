//! `bash` — chạy một lệnh shell.
//!
//! Tool nguy hiểm nhất trong bộ, nên hai quyết định được viết ra đây thay vì để ngầm:
//!
//! **Mặc định phải hỏi.** `meta().mutating` bật, và plugin gắn một canh gác đẩy mọi lời
//! gọi qua `PreDecision::Ask`. Đây là mặc định chứ không phải tuỳ chọn: một tool chạy
//! lệnh mà chưa có đường hỏi người dùng thì không có chế độ an toàn nào để rơi vào.
//!
//! **Không có danh sách đen.** Không lọc `rm -rf`, không lọc `curl | sh`. Một danh sách
//! đen cho lệnh shell luôn thủng — `r''m`, `$(echo cm0K | base64 -d)`, một script trung
//! gian — và cái nó tạo ra không phải an toàn mà là cảm giác an toàn, thứ khiến người ta
//! bấm "cho phép" mà không đọc. Phòng thủ thật là duyệt và sandbox.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::jobs::{JobState, Jobs};
use crate::provider::{Request, ShellExecutor};

/// Không có hạn giờ thì một lệnh chờ nhập liệu sẽ treo cả lượt cho tới khi người dùng bỏ.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BashArgs {
    /// Lệnh cần chạy, diễn giải bởi `/bin/sh -c`.
    pub command: String,
    /// Hạn giờ, mili-giây. Mặc định 120000.
    pub timeout: Option<u64>,
    /// Chạy nền và trả `job_id` ngay. Dùng cho tiến trình sống lâu.
    #[serde(default)]
    pub run_in_background: bool,
}

pub struct Bash {
    shell: Arc<dyn ShellExecutor>,
    jobs: Arc<Jobs>,
    cwd: PathBuf,
}

impl Bash {
    pub const NAME: &'static str = "bash";

    pub fn new(shell: Arc<dyn ShellExecutor>, jobs: Arc<Jobs>, cwd: PathBuf) -> Bash {
        Bash { shell, jobs, cwd }
    }
}

#[async_trait]
impl Tool for Bash {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Bash::NAME,
            "Chạy một lệnh shell trong thư mục làm việc. Với tiến trình sống lâu (máy chủ \
             phát triển, theo dõi tệp), đặt `run_in_background` rồi lấy kết quả bằng \
             `job_output`.",
            json_schema_for::<BashArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Không đánh dấu `concurrency_safe`: hai lệnh chạy song song trong cùng một thư
        // mục làm việc là hai lệnh có thể giẫm lên nhau, và shell không nói cho ai biết.
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
            // Token riêng, không phải token của lượt: job phải sống qua lượt sinh ra nó,
            // nếu không `run_in_background` chẳng khác gì chạy thường.
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

        let meta = json!({
            "command": args.command,
            "cwd": shown_cwd,
            "output": run.output,
            "exit_code": run.exit_code,
            "signal": run.signal,
            "background": false,
            "job_id": serde_json::Value::Null,
        });

        // Mã thoát khác 0 **không** phải `is_error`: lệnh đã chạy đúng như được bảo, và
        // một bộ test đỏ là kết quả hữu ích chứ không phải một lần gọi tool hỏng.
        Ok(ToolOutcome::ok(rendered).with_meta("terminal", meta))
    }
}
