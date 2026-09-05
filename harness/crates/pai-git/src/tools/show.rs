//! `git.show` — one commit in full: its message and the diff it introduced.
//!
//! The companion to `git.log`, which hands out shas and truncated bodies. The split is
//! deliberate: `log` is for scanning, `show` is for the one commit that turned out to matter.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::render::{OVERFLOW_NOTICE, cap_lines, finish};
use crate::repo::{Repo, check_rev};
use crate::tools::read_meta;

/// Default line budget, matching `git.diff`: the body of a `show` is a diff.
const DEFAULT_MAX_LINES: usize = 800;
const MAX_MAX_LINES: usize = 10_000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShowArgs {
    /// Phiên bản cần xem: sha, tên nhánh, tag, `HEAD~2`… Để trống là HEAD.
    pub rev: Option<String>,
    /// Chỉ hiện phần thay đổi của những đường dẫn này (tương đối so với gốc kho).
    pub paths: Option<Vec<String>>,
    /// `true` để chỉ lấy thông điệp commit và bảng tổng kết tệp, bỏ nội dung thay đổi.
    pub stat_only: Option<bool>,
    /// Tối đa bao nhiêu dòng kết quả. Mặc định 800, trần 10000.
    pub max_lines: Option<usize>,
}

pub struct GitShow {
    repo: Arc<Repo>,
    overflow: Overflow,
}

impl GitShow {
    pub const NAME: &'static str = "git.show";

    pub fn new(repo: Arc<Repo>, overflow: Overflow) -> GitShow {
        GitShow { repo, overflow }
    }
}

#[async_trait]
impl Tool for GitShow {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GitShow::NAME,
            "Đọc trọn một commit trong kho git của dự án đang mở: thông điệp đầy đủ và phần \
             thay đổi nó mang lại. Lấy sha từ `git.log`. Với commit lớn, đặt `stat_only: true` \
             trước để xem nó đụng vào những tệp nào, rồi mới gọi lại với `paths` cụ thể.",
            json_schema_for::<ShowArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        read_meta()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ShowArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let max_lines = args
            .max_lines
            .unwrap_or(DEFAULT_MAX_LINES)
            .clamp(1, MAX_MAX_LINES);
        // An absent `rev` means HEAD, the same default git itself uses; naming it explicitly
        // keeps the value we report in `structured` honest.
        let rev = match &args.rev {
            Some(rev) => check_rev(rev)?,
            None => "HEAD".to_string(),
        };

        let mut argv = vec![
            "show".to_string(),
            // Both halves of the same reasoning as `git.diff`: `--no-ext-diff` stops
            // `diff.external`, and `--no-textconv` stops the `textconv` driver it leaves
            // running. No user-configured program gets to run behind a read-only tool.
            "--no-ext-diff".to_string(),
            "--no-textconv".to_string(),
            "--no-color".to_string(),
            "--find-renames".to_string(),
            "--date=iso-strict".to_string(),
        ];
        if args.stat_only.unwrap_or(false) {
            argv.push("--stat".to_string());
        }
        argv.push(rev.clone());
        let paths = match &args.paths {
            Some(paths) => self.repo.relatives(paths)?,
            None => Vec::new(),
        };
        argv.push("--".to_string());
        argv.extend(paths.iter().cloned());

        let out = self.repo.run(&argv, &call.cancel_token()).await?;
        if out.stdout.trim().is_empty() {
            // `show` on a real commit is never empty, so this is a pathspec that matched
            // nothing — worth saying, because "no output" reads like "no changes".
            return Ok(ToolOutcome::ok(format!(
                "`{rev}` không có nội dung nào khớp với `paths` đã cho."
            ))
            .with_structured(json!({ "shape": "git.show", "rev": rev, "empty": true })));
        }

        let capped = cap_lines(&out.stdout, max_lines);
        let mut text = capped.render(
            "Gọi lại `git.show` với `stat_only: true` để xem danh sách tệp, rồi với `paths` \
             để đọc từng tệp.",
        );
        if out.overflowed {
            text.push('\n');
            text.push_str(OVERFLOW_NOTICE);
        }

        let structured = json!({
            "shape": "git.show",
            "rev": rev,
            "paths": paths,
            "stat_only": args.stat_only.unwrap_or(false),
            "lines": capped.total,
            "truncated": capped.truncated(),
        });
        Ok(finish(
            &self.overflow,
            call,
            text,
            structured,
            "Gọi lại `git.show` với `paths` hẹp hơn để đọc trọn vẹn.",
        ))
    }
}
