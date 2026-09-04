//! `code.graph` — the neighbourhood of one symbol.
//! Answers what `symbol_search` cannot: not "where is this function" but "what does
//! changing it touch", which otherwise costs one `grep` and one manual filter per hop.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::index::{DEFAULT_NODES, MAX_DEPTH, SymbolIndex};
use crate::tools::{render_graph, warn_line};

/// One hop is "who touches me", two is "who touches those" — nearly always enough to decide whether to read on.
const DEFAULT_DEPTH: u32 = 2;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GraphArgs {
    /// Tên ký hiệu. Nhận cả dạng đủ tư cách `KieuCha::ten` mà `symbol_search` in ra.
    pub symbol: String,
    /// Đi xa bao nhiêu bước. Mặc định 2, trần 4.
    pub depth: Option<u32>,
}

pub struct CodeGraph {
    index: Arc<dyn SymbolIndex>,
}

impl CodeGraph {
    pub const NAME: &'static str = "code.graph";

    pub fn new(index: Arc<dyn SymbolIndex>) -> CodeGraph {
        CodeGraph { index }
    }
}

#[async_trait]
impl Tool for CodeGraph {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            CodeGraph::NAME,
            "Lấy lát cắt đồ thị quanh một ký hiệu: cái gì chứa nó, nó gọi gì, ai gọi nó, \
             nó cài đặt hay kế thừa cái gì. Dùng trước khi sửa một hàm, để biết chỗ nào \
             vỡ theo. Cạnh được suy ra theo **tên** chứ không theo phân tích kiểu, nên \
             một tên trùng ở nhiều nơi sinh ra nhiều cạnh và một lời gọi động có thể \
             không sinh cạnh nào — kiểm lại bằng `read` trước khi dựa vào nó. Hỗ trợ \
             Rust, TypeScript, JavaScript, Python.",
            json_schema_for::<GraphArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: GraphArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        // Sync before every query, as `symbol_search` does: a stale graph sends the model to the wrong place.
        self.index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let depth = args.depth.unwrap_or(DEFAULT_DEPTH);
        let found = self
            .index
            .neighborhood(&args.symbol, depth, DEFAULT_NODES)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if found.nodes.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có ký hiệu nào tên `{}` trong chỉ mục. `symbol_search` tìm được \
                 theo một phần của tên; chỉ mục chỉ chứa Rust, TypeScript, JavaScript và \
                 Python, có trần kích thước/số tệp, và bỏ qua mã sinh cùng những gì \
                 `.gitignore` loại trừ.",
                args.symbol
            )));
        }

        let (text, meta) = render_graph(&found);
        let head = format!(
            "Lân cận của `{}` — sâu {}, {} đỉnh, {} cạnh.{}",
            args.symbol,
            depth.min(MAX_DEPTH),
            found.nodes.len(),
            found.edges.len(),
            warn_line(found.truncated),
        );
        Ok(ToolOutcome::ok(format!("{head}\n\n{text}")).with_meta("graph", meta))
    }
}
