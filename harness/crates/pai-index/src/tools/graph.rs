//! `code.graph` — lân cận của một ký hiệu.
//!
//! Câu hỏi nó thay thế là câu hỏi mà `symbol_search` không trả lời được: không phải "hàm
//! này ở đâu" mà "sửa hàm này thì đụng vào cái gì". Không có nó, mô hình phải `grep` tên
//! hàm rồi tự lọc chỗ khai báo khỏi chỗ dùng, một lần cho mỗi bước — và nó thường dừng
//! lại sau bước thứ nhất.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::index::{DEFAULT_NODES, MAX_DEPTH, SymbolIndex};
use crate::tools::{render_graph, warn_line};

/// Một bước là "ai chạm vào tôi"; hai bước đã là "ai chạm vào cái chạm vào tôi", và đó
/// gần như luôn là đủ để quyết định có phải đọc tiếp hay không.
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

        // Đồng bộ trước mỗi lần hỏi, cùng lý do với `symbol_search`: một đồ thị nói về mã
        // của mười phút trước là một đồ thị dẫn mô hình đi sai chỗ.
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
                 Python, và bỏ qua những gì `.gitignore` loại trừ.",
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
