//! `git.diff` — what changed, as a unified diff.
//!
//! The tool most likely to blow a context window, so it has the most restraint built in: a
//! line budget the caller can raise but not remove, and `stat_only` for the question that is
//! usually being asked anyway ("which files moved?" rather than "show me every byte").

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

/// Default line budget. Around a thousand lines of diff is already more than a review can
/// hold in mind at once, and the token budget would fold anything much larger anyway.
const DEFAULT_MAX_LINES: usize = 800;
/// Hard ceiling on `max_lines`.
const MAX_MAX_LINES: usize = 10_000;
/// Ceiling on `context`; git's own default is 3, and asking for hundreds of unchanged lines
/// around each hunk is a way of asking for the file, which `read` does better.
const MAX_CONTEXT: u32 = 25;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiffArgs {
    /// Phiên bản làm mốc để so sánh (nhánh, tag, sha, `HEAD~3`…). Để trống là so với cây làm việc hiện tại.
    pub base: Option<String>,
    /// Phiên bản bên kia. Phải đi kèm `base`; để trống thì so `base` với cây làm việc.
    pub head: Option<String>,
    /// `true` để xem phần đã đưa vào chỉ mục (`git diff --staged`). Bị bỏ qua nếu có `base`.
    pub staged: Option<bool>,
    /// Chỉ so những đường dẫn này (tương đối so với gốc kho).
    pub paths: Option<Vec<String>>,
    /// `true` để chỉ lấy bảng tổng kết mỗi tệp thêm/bớt bao nhiêu dòng, không lấy nội dung.
    pub stat_only: Option<bool>,
    /// Số dòng ngữ cảnh quanh mỗi thay đổi. Mặc định 3, trần 25.
    pub context: Option<u32>,
    /// Tối đa bao nhiêu dòng kết quả. Mặc định 800, trần 10000.
    pub max_lines: Option<usize>,
}

pub struct GitDiff {
    repo: Arc<Repo>,
    overflow: Overflow,
}

impl GitDiff {
    pub const NAME: &'static str = "git.diff";

    pub fn new(repo: Arc<Repo>, overflow: Overflow) -> GitDiff {
        GitDiff { repo, overflow }
    }
}

#[async_trait]
impl Tool for GitDiff {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GitDiff::NAME,
            "Xem khác biệt trong kho git của dự án đang mở. Không có tham số nào thì so cây \
             làm việc với chỉ mục; `staged: true` so chỉ mục với HEAD; có `base` thì so với \
             phiên bản đó. Kết quả bị cắt theo `max_lines`, và khi bị cắt sẽ nói rõ còn bao \
             nhiêu — hãy dùng `paths` hoặc `stat_only: true` để thu hẹp thay vì nâng giới hạn.",
            json_schema_for::<DiffArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        read_meta()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: DiffArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let max_lines = args
            .max_lines
            .unwrap_or(DEFAULT_MAX_LINES)
            .clamp(1, MAX_MAX_LINES);

        let mut argv = vec![
            "diff".to_string(),
            // `diff.external` runs a program of the user's choosing. Their terminal is their
            // business; a model-issued call is not, so that door is shut.
            "--no-ext-diff".to_string(),
            // And this is the second door, which `--no-ext-diff` does *not* close: a
            // `diff.<driver>.textconv` command is still run, once per side, whenever
            // `.gitattributes` — a file inside the repository, so a file a contributor
            // writes — points a path at that driver. Worse than the execution, it also lies:
            // with a textconv in play the diff of a changed file can come back empty, because
            // both versions converted to the same text.
            "--no-textconv".to_string(),
            "--no-color".to_string(),
            // Renames read as one move instead of a delete plus an add of the same file.
            "--find-renames".to_string(),
        ];
        if args.stat_only.unwrap_or(false) {
            argv.push("--stat".to_string());
        }
        if let Some(context) = args.context {
            argv.push(format!("-U{}", context.min(MAX_CONTEXT)));
        }

        // `base` wins over `staged`: asking for both is contradictory, and silently mixing
        // them would answer a question nobody asked.
        let mut against = Vec::new();
        match (&args.base, &args.head) {
            (Some(base), Some(head)) => {
                against.push(check_rev(base)?);
                against.push(check_rev(head)?);
            }
            (Some(base), None) => against.push(check_rev(base)?),
            // `head` alone would quietly become "cây làm việc với chỉ mục", and the model
            // would read that answer as the diff of the revision it named. Say no instead:
            // a wrong answer nobody flagged is worse than a call that has to be made twice.
            (None, Some(head)) => {
                return Err(ToolError::Invalid(format!(
                    "có `head` = `{head}` mà không có `base`, nên không rõ so với cái gì. Thêm \
                     `base`, hoặc dùng `git.show` nếu chỉ muốn xem một commit."
                )));
            }
            (None, None) => {
                if args.staged.unwrap_or(false) {
                    argv.push("--staged".to_string());
                }
            }
        }
        argv.extend(against.iter().cloned());

        let paths = match &args.paths {
            Some(paths) => self.repo.relatives(paths)?,
            None => Vec::new(),
        };
        // Always emit `--`, even with no paths: it settles the ambiguity between a revision
        // and a filename of the same name, which is a real and confusing failure mode.
        argv.push("--".to_string());
        argv.extend(paths.iter().cloned());

        let out = self.repo.run(&argv, &call.cancel_token()).await?;
        if out.stdout.trim().is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có khác biệt nào ({}).",
                describe(&args, &against)
            ))
            .with_structured(json!({ "shape": "git.diff", "empty": true })));
        }

        let capped = cap_lines(&out.stdout, max_lines);
        let mut text = capped.render(
            "Gọi lại `git.diff` với `paths` để xem từng tệp, hoặc `stat_only: true` để chỉ \
             lấy bảng tổng kết.",
        );
        if out.overflowed {
            text.push('\n');
            text.push_str(OVERFLOW_NOTICE);
        }

        let structured = json!({
            "shape": "git.diff",
            "against": against,
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
            "Gọi lại `git.diff` với `paths` hẹp hơn để đọc trọn vẹn từng phần.",
        ))
    }
}

/// What the empty answer was empty *about*; "no difference" with no subject is not an answer.
fn describe(args: &DiffArgs, against: &[String]) -> String {
    match against {
        [base, head] => format!("giữa `{base}` và `{head}`"),
        [base] => format!("giữa `{base}` và cây làm việc"),
        _ if args.staged.unwrap_or(false) => "giữa chỉ mục và HEAD".to_string(),
        _ => "giữa cây làm việc và chỉ mục".to_string(),
    }
}
