//! `docs.list` — thư viện này có những gì.
//!
//! Tool rẻ nhất trong ba cái, và là tool mô hình nên gọi đầu tiên trong một dự án tài
//! liệu: nó cho biết có bao nhiêu tài liệu, tên chúng là gì, và **phần ngữ nghĩa đã sẵn
//! sàng chưa**. Câu cuối là lý do nó in cả `stats().reason` ra: khi vector chưa có, một
//! mô hình biết điều đó sẽ hỏi `docs.search` bằng từ khoá cụ thể thay vì bằng một câu
//! diễn giải mà chỉ tìm theo ý nghĩa mới hiểu được.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocsListArgs {}

pub struct DocsList {
    docs: Arc<dyn DocLibrary>,
}

impl DocsList {
    pub const NAME: &'static str = "docs.list";

    pub fn new(docs: Arc<dyn DocLibrary>) -> DocsList {
        DocsList { docs }
    }
}

#[async_trait]
impl Tool for DocsList {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            DocsList::NAME,
            "Liệt kê tài liệu trong thư viện của dự án: mã, tên, định dạng và số đoạn. \
             Dùng mã ở đây cho `docs.read`. Kết quả cũng nói phần tìm theo ý nghĩa đã sẵn \
             sàng chưa — khi chưa, hãy hỏi `docs.search` bằng từ khoá cụ thể.",
            json_schema_for::<DocsListArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let documents = self
            .docs
            .documents()
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let stats = self
            .docs
            .stats()
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if documents.is_empty() {
            // Kể cả lời báo trống cũng phải nói **vì sao** trống: thư viện là thư mục dự
            // án, nên câu trả lời gần như luôn nằm ở thư mục đó — chưa có tệp nào đọc
            // được, hay thư mục không mở được. `stats().reason` đã dựng sẵn câu ấy.
            let mut lines = vec![
                "Thư viện tài liệu của dự án này đang trống. Không có tool nào nạp tài liệu \
                 được — thư viện là thư mục của dự án, và người dùng thêm tệp bằng cách đặt \
                 tệp vào đó."
                    .to_string(),
            ];
            if let Some(reason) = stats.reason {
                lines.push(String::new());
                lines.push(reason);
            }
            return Ok(ToolOutcome::ok(lines.join("\n")));
        }

        let mut lines = Vec::with_capacity(documents.len() + 1);
        for doc in &documents {
            let note = match (&doc.error, doc.embedded) {
                (Some(error), _) => format!(" — chưa nhúng: {error}"),
                (None, false) => " — đang chờ nhúng".to_string(),
                (None, true) => String::new(),
            };
            lines.push(format!(
                "{}  {}  [{}, {} đoạn]{note}",
                doc.id,
                doc.title,
                doc.format.as_str(),
                doc.chunks
            ));
        }
        if let Some(reason) = &stats.reason {
            lines.push(String::new());
            lines.push(reason.clone());
        }

        let meta = json!({
            "shape": "documents",
            "semantic_ready": stats.semantic_ready,
            "documents": documents.iter().map(|doc| json!({
                "id": doc.id,
                "title": doc.title,
                "path": doc.path.display().to_string(),
                "format": doc.format.as_str(),
                "bytes": doc.bytes,
                "chunks": doc.chunks,
                "embedded": doc.embedded,
                "addedAt": doc.added_at,
                "error": doc.error,
            })).collect::<Vec<_>>(),
        });
        Ok(ToolOutcome::ok(lines.join("\n")).with_meta("documents", meta))
    }
}
