//! `outline` — a file's symbol map.
//! A two-thousand-line file costs about twenty thousand tokens to read; usually the model
//! only needs to know what is in it to pick thirty lines. The map costs a few hundred.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use pai_fs::FileRoots;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::index::SymbolIndex;
use crate::symbol::Symbol;

/// How many lines the UI card shows; the remainder still goes out in the content.
const DISPLAY_CAP: usize = 250;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OutlineArgs {
    /// Đường dẫn tới tệp cần xem cấu trúc.
    pub file_path: String,
}

pub struct Outline {
    index: Arc<dyn SymbolIndex>,
    roots: FileRoots,
}

impl Outline {
    pub const NAME: &'static str = "outline";

    pub fn new(index: Arc<dyn SymbolIndex>, roots: FileRoots) -> Outline {
        Outline { index, roots }
    }
}

/// Nested items are indented; one level is enough, as the index stores only the direct parent.
fn line(symbol: &Symbol) -> String {
    let indent = if symbol.parent.is_some() { "  " } else { "" };
    format!(
        "{indent}{}-{} {} {}",
        symbol.start_line,
        symbol.end_line,
        symbol.kind.as_str(),
        symbol.name
    )
}

#[async_trait]
impl Tool for Outline {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Outline::NAME,
            "Liệt kê hàm, kiểu, trait và hằng của một tệp kèm khoảng dòng của từng cái, \
             không đọc nội dung. Dùng nó trước khi `read` một tệp dài để biết nên đọc \
             đoạn nào. Hỗ trợ Rust, TypeScript, JavaScript, Python.",
            json_schema_for::<OutlineArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: OutlineArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        // Resolve then check, exactly as `read` does: an index that answers about files outside the roots bypasses them.
        let path = self
            .roots
            .resolve_read(Path::new(&args.file_path))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        self.index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let found = self
            .index
            .outline(&path)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let display = path.display().to_string();
        let Some(symbols) = found else {
            return Ok(ToolOutcome::ok(format!(
                "{display} không nằm trong chỉ mục: ngôn ngữ có thể chưa được hỗ trợ, \
                 tệp có thể vượt trần, hoặc bộ lọc dự án đã loại nó ra. Đọc thẳng bằng `read`."
            )));
        };
        if symbols.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "{display} không khai báo hàm, kiểu, trait hay hằng nào."
            )));
        }

        let rendered = symbols.iter().map(line).collect::<Vec<_>>().join("\n");
        let matches: Vec<serde_json::Value> = symbols
            .iter()
            .take(DISPLAY_CAP)
            .map(|symbol| json!({ "line": symbol.start_line, "text": line(symbol) }))
            .collect();
        let meta = json!({
            "shape": "matches",
            "truncated": symbols.len() > DISPLAY_CAP,
            "total": symbols.len(),
            "groups": [{ "path": display, "matches": matches }],
        });

        Ok(ToolOutcome::ok(rendered).with_meta("search", meta))
    }
}
