//! `docs.read` — đọc liền mạch một tài liệu, theo đoạn.
//!
//! `docs.search` trả về những mảnh rời rạc; tool này là đường để mô hình đọc phần trước
//! sau của một mảnh. Phân trang theo **số thứ tự đoạn** chứ không theo dòng hay theo byte,
//! vì đoạn là đơn vị mà `docs.search` vừa trích dẫn: mô hình thấy `#12` trong kết quả tìm
//! và hỏi tiếp từ `offset: 10` mà không phải quy đổi gì cả.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;
use crate::tools::render;

/// Sáu đoạn ~1000 ký tự một lần đọc. Đọc cả một tài liệu trăm trang trong một lời gọi thì
/// phần đầu đã ra khỏi cửa sổ ngữ cảnh trước khi mô hình dùng tới phần cuối.
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
}

impl DocsRead {
    pub const NAME: &'static str = "docs.read";

    pub fn new(docs: Arc<dyn DocLibrary>) -> DocsRead {
        DocsRead { docs }
    }
}

#[async_trait]
impl Tool for DocsRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            DocsRead::NAME,
            "Đọc một tài liệu trong thư viện theo thứ tự, từng đoạn một. Dùng nó sau \
             `docs.search` để xem phần trước và sau của một đoạn đã tìm được; `offset` \
             đếm theo số thứ tự đoạn mà `docs.search` đã in ra.",
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
                "Tài liệu `{}` không có đoạn nào từ vị trí {offset}. Dùng `docs.list` để \
                 xem tài liệu có bao nhiêu đoạn.",
                args.document_id
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
