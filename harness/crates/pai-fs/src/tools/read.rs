//! `read` — đọc một tệp, có đánh số dòng.
//!
//! Số dòng không phải để trang trí: `edit` khớp theo chuỗi nguyên văn, nên mô hình cần
//! biết nó đang nhìn dòng nào để trích đúng đoạn cần thay. Không đánh số thì nó đếm, và
//! nó đếm sai.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::observed::ReadLedger;
use crate::path::FileRoots;
use crate::provider::FsProvider;

/// Đọc quá nhiều dòng một lúc thì phần đầu bị đẩy ra khỏi cửa sổ ngữ cảnh trước khi mô
/// hình dùng tới. Hai nghìn là chỗ dsh dừng lại, và không có lý do gì để khác.
const DEFAULT_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Đường dẫn tuyệt đối tới tệp cần đọc.
    pub file_path: String,
    /// Dòng bắt đầu, đếm từ 1. Bỏ trống là đọc từ đầu.
    pub offset: Option<usize>,
    /// Số dòng tối đa. Bỏ trống là 2000.
    pub limit: Option<usize>,
}

pub struct Read {
    fs: Arc<dyn FsProvider>,
    roots: FileRoots,
    ledger: Arc<ReadLedger>,
}

impl Read {
    pub const NAME: &'static str = "read";

    pub fn new(fs: Arc<dyn FsProvider>, roots: FileRoots, ledger: Arc<ReadLedger>) -> Read {
        Read { fs, roots, ledger }
    }
}

#[async_trait]
impl Tool for Read {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Read::NAME,
            "Đọc một tệp văn bản trên đĩa. Kết quả có đánh số dòng. Mặc định đọc 2000 \
             dòng đầu; dùng `offset` và `limit` để đọc tiếp phần sau.",
            json_schema_for::<ReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Nội dung tệp là dữ liệu của người dùng, không phải chỉ dẫn cho mô hình — kể cả
        // khi tệp đó chứa một câu trông rất giống chỉ dẫn.
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

        // Ghi nhận *sau* khi đọc thành công: một lần đọc hỏng không mở khoá cho `edit`.
        self.ledger.note_read(&resolved);

        let meta = json!({
            "path": resolved.display().to_string(),
            "offset": start + 1,
            "lines": lines,
            "total_lines": total,
            "lang": resolved.extension().and_then(|e| e.to_str()),
        });
        Ok(ToolOutcome::ok(rendered).with_meta("read", meta))
    }
}
