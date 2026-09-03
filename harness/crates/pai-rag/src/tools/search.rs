//! `docs.search` — tìm đoạn trong thư viện tài liệu.
//!
//! Đây là tool mà mô hình gọi trước khi trả lời gần như mọi câu hỏi trong một dự án tài
//! liệu. Mô tả của nó nói thẳng rằng tìm kiếm là **lai ghép** và có thể đang chạy thiếu
//! nửa ngữ nghĩa, vì một mô hình biết mình chỉ có từ khoá sẽ hỏi lại bằng từ khoá thay vì
//! kết luận rằng tài liệu không nhắc tới chuyện đó.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;
use crate::tools::render;

/// Bao nhiêu đoạn khi mô hình không nói gì. Tám đoạn ~1000 ký tự là khoảng 2500 token —
/// đủ để trả lời một câu hỏi, chưa đủ để đẩy phần còn lại của hội thoại ra ngoài cửa sổ.
const DEFAULT_LIMIT: usize = 8;
/// Trần cứng. Xin ba mươi đoạn là đang dùng tool này thay cho `docs.read`.
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
}

impl DocsSearch {
    pub const NAME: &'static str = "docs.search";

    pub fn new(docs: Arc<dyn DocLibrary>) -> DocsSearch {
        DocsSearch { docs }
    }
}

#[async_trait]
impl Tool for DocsSearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            DocsSearch::NAME,
            "Tìm những đoạn liên quan trong thư viện tài liệu của dự án. Kết hợp tìm theo \
             từ khoá với tìm theo ý nghĩa, nên hỏi bằng cả một câu cũng được. Mỗi kết quả \
             mang tên tài liệu và số thứ tự đoạn — hãy trích dẫn chúng khi trả lời, và \
             dùng `docs.read` để đọc thêm phần trước sau của một đoạn.",
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
            // Nói ra vì sao rỗng. "Không tìm thấy" trong một thư viện chưa nhúng xong là
            // một câu trả lời khác hẳn với "không tìm thấy" trong một thư viện đầy đủ, và
            // mô hình chỉ hỏi lại đúng cách khi nó biết mình đang ở trường hợp nào.
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
