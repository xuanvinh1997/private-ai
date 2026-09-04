//! `job_output`, `job_kill`, `job_list`: see and stop what is running in the background.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::jobs::{JobState, Jobs};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobRef {
    /// The id `bash` returns when backgrounding.
    pub job_id: String,
}

pub struct JobOutput {
    jobs: Arc<Jobs>,
}

impl JobOutput {
    pub const NAME: &'static str = "job_output";
    pub fn new(jobs: Arc<Jobs>) -> JobOutput {
        JobOutput { jobs }
    }
}

#[async_trait]
impl Tool for JobOutput {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            JobOutput::NAME,
            "Lấy output của một lệnh đang hoặc đã chạy nền.",
            json_schema_for::<JobRef>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: JobRef =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let job = self
            .jobs
            .get(&args.job_id)
            .ok_or_else(|| ToolError::Invalid(format!("không có job `{}`", args.job_id)))?;

        let state = job.state.lock().clone();
        let (text, meta) = match state {
            // Still running is not the same as hung, and the answer has to say so.
            JobState::Running => (
                format!("Job `{}` vẫn đang chạy.", job.id),
                json!({ "exit_code": serde_json::Value::Null, "background": true }),
            ),
            JobState::Finished(run) => {
                let mut text = run.output.clone();
                if let Some(reason) = &run.interrupted {
                    text.push_str(&format!("\n[{reason}]\n"));
                }
                if text.trim().is_empty() {
                    text = "(không có output)".to_string();
                }
                (
                    text,
                    json!({ "exit_code": run.exit_code, "background": true }),
                )
            }
        };

        let meta = json!({
            "command": job.command,
            "cwd": job.cwd,
            "output": text,
            "exit_code": meta["exit_code"],
            "background": true,
            "job_id": job.id,
        });
        Ok(ToolOutcome::ok(text).with_meta("terminal", meta))
    }
}

pub struct JobKill {
    jobs: Arc<Jobs>,
}

impl JobKill {
    pub const NAME: &'static str = "job_kill";
    pub fn new(jobs: Arc<Jobs>) -> JobKill {
        JobKill { jobs }
    }
}

#[async_trait]
impl Tool for JobKill {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            JobKill::NAME,
            "Dừng một lệnh đang chạy nền, cùng toàn bộ tiến trình con của nó.",
            json_schema_for::<JobRef>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::mutating().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: JobRef =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        if self.jobs.kill(&args.job_id) {
            Ok(ToolOutcome::ok(format!("Đã dừng job `{}`.", args.job_id)))
        } else {
            Err(ToolError::Invalid(format!(
                "không có job `{}`",
                args.job_id
            )))
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

pub struct JobList {
    jobs: Arc<Jobs>,
}

impl JobList {
    pub const NAME: &'static str = "job_list";
    pub fn new(jobs: Arc<Jobs>) -> JobList {
        JobList { jobs }
    }
}

#[async_trait]
impl Tool for JobList {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            JobList::NAME,
            "Liệt kê những lệnh đang chạy nền.",
            json_schema_for::<NoArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().concurrency_safe(true)
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let jobs = self.jobs.list();
        if jobs.is_empty() {
            return Ok(ToolOutcome::ok("Không có job nào đang chạy nền."));
        }
        let rendered = jobs
            .iter()
            .map(|job| {
                let state = match &*job.state.lock() {
                    JobState::Running => "đang chạy".to_string(),
                    JobState::Finished(run) => match run.exit_code {
                        Some(code) => format!("xong, mã thoát {code}"),
                        None => "đã dừng".to_string(),
                    },
                };
                format!("{}  {}  ({state})", job.id, job.command)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ToolOutcome::ok(rendered))
    }
}
