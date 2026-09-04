//! `code.overview` — the architecture map, read before reading code.
//! Same idea as `outline` one level up: not a directory tree, which `glob` already gives,
//! but the densest directories and the busiest symbols — where to start reading.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::graph::NAME_BASED_NOTICE;
use crate::index::SymbolIndex;

/// No parameters; an empty struct rather than no schema, since an empty object schema tells the model to send nothing.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OverviewArgs {}

pub struct CodeOverview {
    index: Arc<dyn SymbolIndex>,
}

impl CodeOverview {
    pub const NAME: &'static str = "code.overview";

    pub fn new(index: Arc<dyn SymbolIndex>) -> CodeOverview {
        CodeOverview { index }
    }
}

#[async_trait]
impl Tool for CodeOverview {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            CodeOverview::NAME,
            "Bản đồ kiến trúc của thư mục làm việc: thư mục nào chứa bao nhiêu tệp và ký \
             hiệu, ngôn ngữ nào chiếm bao nhiêu tệp, và những ký hiệu có nhiều quan hệ \
             nhất — tức là chỗ đáng đọc trước. Gọi nó ở đầu một việc trên kho lạ, thay \
             cho một chuỗi `glob` và `read` để dò đường. Bậc của một ký hiệu được đếm \
             trên những cạnh suy ra theo tên, nên nó là thứ tự gợi ý chứ không phải một \
             phép đo.",
            json_schema_for::<OverviewArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        self.index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let map = self
            .index
            .overview()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let stats = self
            .index
            .stats()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if stats.files == 0 {
            return Ok(ToolOutcome::ok(
                "Chỉ mục rỗng: không có tệp Rust, TypeScript, JavaScript hay Python nào \
                 trong thư mục làm việc, hoặc `.gitignore` đã loại hết chúng."
                    .to_string(),
            ));
        }

        let mut lines = vec![format!(
            "{} tệp, {} ký hiệu, {} cạnh.",
            stats.files, stats.symbols, stats.edges
        )];
        lines.push(
            stats
                .languages
                .iter()
                .map(|(lang, count)| format!("{lang} {count}"))
                .collect::<Vec<_>>()
                .join(", "),
        );

        lines.push(String::new());
        lines.push("thư mục (nhiều ký hiệu trước):".to_string());
        for folder in &map.directories {
            lines.push(format!(
                "{} — {} tệp, {} ký hiệu",
                folder.path, folder.files, folder.symbols
            ));
        }
        if map.directories_omitted > 0 {
            lines.push(format!(
                "… và {} thư mục nữa không kể ra.",
                map.directories_omitted
            ));
        }

        lines.push(String::new());
        lines.push("ký hiệu nhiều quan hệ nhất:".to_string());
        for central in &map.central {
            lines.push(format!(
                "{}:{} {} {} — {} vào, {} ra",
                central.node.path,
                central.node.line,
                central.node.kind,
                central.node.name,
                central.incoming,
                central.outgoing
            ));
        }

        lines.push(String::new());
        lines.push(NAME_BASED_NOTICE.to_string());

        let meta = json!({
            "shape": "overview",
            "stats": {
                "files": stats.files,
                "symbols": stats.symbols,
                "edges": stats.edges,
                "languages": stats.languages,
                "scannedAt": stats.scanned_at,
            },
            "directories": map.directories.iter().map(|folder| json!({
                "path": folder.path,
                "files": folder.files,
                "symbols": folder.symbols,
            })).collect::<Vec<_>>(),
            "central": map.central.iter().map(|central| json!({
                "id": central.node.id.to_string(),
                "name": central.node.name,
                "kind": central.node.kind,
                "path": central.node.path,
                "line": central.node.line,
                "incoming": central.incoming,
                "outgoing": central.outgoing,
            })).collect::<Vec<_>>(),
        });

        Ok(ToolOutcome::ok(lines.join("\n")).with_meta("overview", meta))
    }
}
