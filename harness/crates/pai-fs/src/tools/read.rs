//! `read`: read a file with line numbers, which `edit` needs so the model can quote the
//! right stretch instead of counting lines itself and getting it wrong.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::observed::ReadLedger;
use crate::path::FileRoots;
use crate::provider::FsProvider;

/// Caller default, not a ceiling: the token budget is the real ceiling and is independent.
const DEFAULT_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Absolute path of the file to read.
    pub file_path: String,
    /// The first line, counting from 1. Empty reads from the start.
    pub offset: Option<usize>,
    /// Maximum number of lines. Defaults to 2000.
    pub limit: Option<usize>,
}

pub struct Read {
    fs: Arc<dyn FsProvider>,
    roots: FileRoots,
    ledger: Arc<ReadLedger>,
    overflow: Overflow,
}

impl Read {
    pub const NAME: &'static str = "read";

    pub fn new(
        fs: Arc<dyn FsProvider>,
        roots: FileRoots,
        ledger: Arc<ReadLedger>,
        overflow: Overflow,
    ) -> Read {
        Read {
            fs,
            roots,
            ledger,
            overflow,
        }
    }
}

#[async_trait]
impl Tool for Read {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Read::NAME,
            "Đọc một tệp văn bản trên đĩa. Kết quả có đánh số dòng. Mặc định đọc 2000 \
             dòng đầu; dùng `offset` và `limit` để đọc tiếp phần sau. Kết quả quá dài bị \
             gấp lại thành phần đầu và phần cuối, kèm chỉ dẫn đọc tiếp — không có gì bị \
             vứt đi.",
            json_schema_for::<ReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // File contents are data, never instructions, however much they read like one.
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ReadArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let resolved = self
            .roots
            .resolve_read(Path::new(&args.file_path))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let text = self
            .fs
            .read_text(&resolved)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let all: Vec<&str> = text.lines().collect();
        let total = all.len();
        let start = args.offset.unwrap_or(1).max(1) - 1;
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).max(1);
        let slice = all.iter().skip(start).take(limit);

        let mut rendered = String::new();
        let mut lines = Vec::new();
        for (offset, line) in slice.enumerate() {
            let number = start + offset + 1;
            rendered.push_str(&format!("{number:>6}\t{line}\n"));
            lines.push(json!({ "number": number, "text": line }));
        }

        if rendered.is_empty() {
            rendered = format!("(tệp có {total} dòng; không có dòng nào trong khoảng đã hỏi)\n");
        }

        // Recorded after a successful read: a failed read does not unlock `edit`.
        self.ledger.note_read(&resolved);

        // Budget applies after `offset`/`limit`, and the next offset counts whole head lines only.
        let folded = self.overflow.fold(&call.name, rendered, |split| {
            format!(
                "Đọc tiếp bằng `read` với `file_path` như cũ và `offset: {}`.",
                start + split.head_lines + 1
            )
        });

        let meta = json!({
            "path": resolved.display().to_string(),
            "offset": start + 1,
            "lines": lines,
            "total_lines": total,
            "lang": resolved.extension().and_then(|e| e.to_str()),
        });
        let mut outcome = ToolOutcome::ok(folded.content).with_meta("read", meta);
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
        Ok(outcome)
    }
}
