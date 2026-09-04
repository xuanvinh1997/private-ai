//! `docs.list` - what this library holds.
//! The cheapest of the three tools and the one to call first in a document project: it
//! also prints `stats().reason`, so the model knows whether the semantic half is ready.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::library::DocLibrary;
use crate::tools::Vocab;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocsListArgs {}

pub struct DocsList {
    docs: Arc<dyn DocLibrary>,
    ten: Vocab,
}

impl DocsList {
    pub fn new(docs: Arc<dyn DocLibrary>, ten: Vocab) -> DocsList {
        DocsList { docs, ten }
    }
}

#[async_trait]
impl Tool for DocsList {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.ten.list,
            format!(
                "Liệt kê {}: mã, tên, định dạng và số đoạn. Dùng mã ở đây cho `{}`. Kết quả cũng nói phần tìm \
                 theo ý nghĩa đã sẵn sàng chưa — khi chưa, hãy hỏi `{}` bằng từ khoá cụ thể.",
                self.ten.what, self.ten.read, self.ten.search
            ),
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
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let stats = self
            .docs
            .stats()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if documents.is_empty() {
            // Even the empty report must say *why* it is empty; `stats().reason` already has that sentence.
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
