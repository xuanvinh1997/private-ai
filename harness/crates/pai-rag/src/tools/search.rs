//! `docs.search` - find chunks in the document library.
//! The tool the model calls before answering almost anything in a document project; its
//! description says search is hybrid and may be running without the semantic half.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;
use crate::tools::Vocab;
use crate::tools::render;

/// Default chunk count. Eight ~1000-character chunks is about 2500 tokens: enough to answer, not enough to evict the conversation.
const DEFAULT_LIMIT: usize = 8;
/// Hard cap. Asking for thirty chunks means using this tool instead of `docs.read`.
const MAX_LIMIT: usize = 30;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocsSearchArgs {
    /// Câu hỏi hoặc cụm từ khoá, viết bằng ngôn ngữ của tài liệu.
    pub query: String,
    /// Tối đa bao nhiêu đoạn. Mặc định 8, trần 30.
    pub limit: Option<usize>,
}

pub struct DocsSearch {
    docs: Arc<dyn DocLibrary>,
    ten: Vocab,
}

impl DocsSearch {
    pub fn new(docs: Arc<dyn DocLibrary>, ten: Vocab) -> DocsSearch {
        DocsSearch { docs, ten }
    }
}

#[async_trait]
impl Tool for DocsSearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.ten.search,
            format!(
                "Tìm những đoạn liên quan trong {}. Kết hợp tìm theo từ khoá với tìm theo ý nghĩa, nên hỏi \
                 bằng cả một câu cũng được. Mỗi kết quả mang tên {} và số thứ tự đoạn — hãy trích dẫn chúng \
                 khi trả lời, và dùng `{}` để đọc thêm phần trước sau của một đoạn.",
                self.ten.what, self.ten.item, self.ten.read
            ),
            json_schema_for::<DocsSearchArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: DocsSearchArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

        let hits = self
            .docs
            .search(&args.query, limit)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let stats = self
            .docs
            .stats()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if hits.is_empty() {
            // Say why the result is empty: "nothing found" in a half-embedded library is a different answer from "nothing found" in a complete one.
            let mut text = format!(
                "Không có đoạn nào khớp `{}` trong {} tài liệu của thư viện.",
                args.query, stats.documents
            );
            if let Some(reason) = &stats.reason {
                text.push_str("\n\n");
                text.push_str(reason);
            }
            return Ok(ToolOutcome::ok(text));
        }

        let rendered = hits.iter().map(render).collect::<Vec<_>>().join("\n\n");
        let meta = json!({
            "shape": "documents",
            "semantic_ready": stats.semantic_ready,
            "hits": hits.iter().map(|hit| json!({
                "documentId": hit.document_id,
                "title": hit.title,
                "path": hit.path.display().to_string(),
                "ordinal": hit.ordinal,
                "text": hit.text,
                "score": hit.score,
                "matchedBy": hit.matched_by.as_str(),
            })).collect::<Vec<_>>(),
        });
        Ok(ToolOutcome::ok(rendered).with_meta("documents", meta))
    }
}
