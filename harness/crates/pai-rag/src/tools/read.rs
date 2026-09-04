//! `docs.read` - read one document straight through, chunk by chunk.
//! `docs.search` returns scattered fragments; this is how the model reads around one.
//! Paging is by chunk ordinal, the same unit `docs.search` just cited.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;
use crate::tools::{Vocab, render};

/// Six ~1000-character chunks per read; a whole hundred-page document would fall out of the context window.
const DEFAULT_LIMIT: usize = 6;
const MAX_LIMIT: usize = 30;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocsReadArgs {
    /// Mã tài liệu, lấy từ `docs.search` hoặc `docs.list`.
    pub document_id: String,
    /// Bắt đầu từ đoạn thứ mấy, đếm từ 0. Bỏ trống là từ đầu.
    pub offset: Option<usize>,
    /// Đọc bao nhiêu đoạn. Mặc định 6, trần 30.
    pub limit: Option<usize>,
}

pub struct DocsRead {
    docs: Arc<dyn DocLibrary>,
    ten: Vocab,
}

impl DocsRead {
    pub fn new(docs: Arc<dyn DocLibrary>, ten: Vocab) -> DocsRead {
        DocsRead { docs, ten }
    }
}

#[async_trait]
impl Tool for DocsRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.ten.read,
            format!(
                "Đọc một {} theo thứ tự, từng đoạn một. Dùng nó sau `{}` để xem phần trước và sau của một \
                 đoạn đã tìm được; `offset` đếm theo số thứ tự đoạn mà `{}` đã in ra.",
                self.ten.item, self.ten.search, self.ten.search
            ),
            json_schema_for::<DocsReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: DocsReadArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let offset = args.offset.unwrap_or(0);
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

        let hits = self
            .docs
            .chunks(&args.document_id, offset, limit)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if hits.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "`{}` không có đoạn nào từ vị trí {offset}. Dùng `{}` để xem nó có bao nhiêu đoạn.",
                args.document_id, self.ten.list
            )));
        }

        let rendered = hits.iter().map(render).collect::<Vec<_>>().join("\n\n");
        let meta = json!({
            "shape": "documents",
            "documentId": args.document_id,
            "offset": offset,
            "count": hits.len(),
        });
        Ok(ToolOutcome::ok(rendered).with_meta("documents", meta))
    }
}
