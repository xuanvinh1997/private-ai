//! `code.trace` — các đường đi theo cạnh `calls`.
//!
//! Khác `code.graph` ở một chỗ và đó là chỗ quan trọng: nó trả về **đường đi**, không
//! phải một tập hợp. "Ai gọi `resolve_read`" trả lời được bằng một tập; "giá trị này từ
//! đâu tới đây" thì không — cần biết nó đi qua những hàm nào, theo thứ tự nào.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::graph::{GraphNode, NAME_BASED_NOTICE};
use crate::index::{MAX_DEPTH, MAX_PATHS, SymbolIndex};

const DEFAULT_DEPTH: u32 = 3;

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Ngược mũi tên: ai gọi ký hiệu này.
    Callers,
    /// Xuôi mũi tên: ký hiệu này gọi ai.
    Callees,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceArgs {
    /// Tên ký hiệu. Nhận cả dạng đủ tư cách `KieuCha::ten`.
    pub symbol: String,
    /// `callers` để đi ngược, `callees` để đi xuôi.
    pub direction: Direction,
    /// Dài tối đa bao nhiêu bước. Mặc định 3, trần 4.
    pub depth: Option<u32>,
}

pub struct CodeTrace {
    index: Arc<dyn SymbolIndex>,
}

impl CodeTrace {
    pub const NAME: &'static str = "code.trace";

    pub fn new(index: Arc<dyn SymbolIndex>) -> CodeTrace {
        CodeTrace { index }
    }
}

fn render_path(path: &[GraphNode]) -> String {
    path.iter()
        .map(|node| format!("{} ({}:{})", node.name, node.path, node.line))
        .collect::<Vec<_>>()
        .join(" → ")
}

#[async_trait]
impl Tool for CodeTrace {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            CodeTrace::NAME,
            "Đi theo cạnh gọi hàm từ một ký hiệu và trả về các **đường đi**, không phải \
             một danh sách. `callers` trả lời \"sửa hàm này thì ai chịu ảnh hưởng\", \
             `callees` trả lời \"hàm này thật ra làm gì\". Cạnh gọi được suy ra theo tên \
             chứ không theo phân tích kiểu: một lời gọi qua biến, qua trait object hay \
             qua con trỏ hàm sẽ không có mặt, và một tên trùng sẽ sinh ra nhánh thừa.",
            json_schema_for::<TraceArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: TraceArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        self.index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let depth = args.depth.unwrap_or(DEFAULT_DEPTH);
        let paths = match args.direction {
            Direction::Callers => self.index.callers(&args.symbol, depth).await,
            Direction::Callees => self.index.callees(&args.symbol, depth).await,
        }
        .map_err(|err| ToolError::Failed(err.to_string()))?;

        let huong = match args.direction {
            Direction::Callers => "gọi tới",
            Direction::Callees => "được gọi từ",
        };
        if paths.is_empty() {
            // "Không tìm thấy đường đi nào" tuyệt đối không được đọc thành "không ai gọi
            // hàm này": với một đồ thị suy đoán theo tên, hai câu đó khác hẳn nhau.
            return Ok(ToolOutcome::ok(format!(
                "Không có đường {huong} nào từ `{}` trong đồ thị. Điều đó **không** có \
                 nghĩa là không tồn tại: {NAME_BASED_NOTICE}",
                args.symbol
            )));
        }

        let mut lines: Vec<String> = paths.iter().map(|path| render_path(path)).collect();
        let truncated = lines.len() >= MAX_PATHS;
        if truncated {
            lines.push(format!(
                "… đã dừng ở {MAX_PATHS} đường đi; thu hẹp bằng `depth` nhỏ hơn."
            ));
        }
        let head = format!(
            "{} đường {huong} `{}`, sâu tối đa {}.",
            paths.len(),
            args.symbol,
            depth.clamp(1, MAX_DEPTH)
        );
        let body = lines.join("\n");

        let meta = json!({
            "shape": "paths",
            "truncated": truncated,
            "total": paths.len(),
            "paths": paths
                .iter()
                .map(|path| path
                    .iter()
                    .map(|node| json!({
                        "id": node.id.to_string(),
                        "name": node.name,
                        "kind": node.kind,
                        "path": node.path,
                        "line": node.line,
                    }))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        });
        Ok(
            ToolOutcome::ok(format!("{head}\n{body}\n\n{NAME_BASED_NOTICE}"))
                .with_meta("trace", meta),
        )
    }
}
