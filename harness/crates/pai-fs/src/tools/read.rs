//! `read` — đọc một tệp, có đánh số dòng.
//!
//! Số dòng không phải để trang trí: `edit` khớp theo chuỗi nguyên văn, nên mô hình cần
//! biết nó đang nhìn dòng nào để trích đúng đoạn cần thay. Không đánh số thì nó đếm, và
//! nó đếm sai.

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

/// Đọc quá nhiều dòng một lúc thì phần đầu bị đẩy ra khỏi cửa sổ ngữ cảnh trước khi mô
/// hình dùng tới. Hai nghìn là chỗ dsh dừng lại, và không có lý do gì để khác.
///
/// Đây là **lựa chọn mặc định của người gọi**, không phải trần. Trần là ngân sách token,
/// và hai thứ đó độc lập: một tệp JSON tối giản 100 dòng vẫn vượt ngân sách trong khi một
/// tệp Rust 2000 dòng thưa thì không, nên `limit` không thay được cho việc đo.
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

        // Ngân sách áp **sau** `offset`/`limit`: hai thứ đó là ý muốn của người gọi, còn
        // ngân sách là trần trên của kết quả. Số dòng lấy tiếp tính từ số dòng trọn vẹn
        // trong phần đầu — một dòng bị cắt làm đôi chưa được coi là đã đọc.
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
