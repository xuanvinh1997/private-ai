//! `outline` — bản đồ ký hiệu của một tệp.
//!
//! Lý do tồn tại: một tệp hai nghìn dòng tốn khoảng hai mươi nghìn token để đọc, và
//! thường thì mô hình chỉ cần biết trong đó có gì để chọn ra ba mươi dòng đáng đọc. Cái
//! bản đồ đó tốn vài trăm token.

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

/// Bao nhiêu dòng được hiện trong thẻ giao diện. Phần dư vẫn nằm nguyên trong content.
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

/// Lồng nhau thì thụt vào. Một tầng là đủ: quan hệ duy nhất chỉ mục lưu là cha trực tiếp.
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

        // Chuẩn hoá trước, kiểm tra sau — và luật đó áp cho đường dẫn của tool này y hệt
        // như cho `read`. Một chỉ mục trả lời được câu hỏi "tệp ngoài gốc có những hàm
        // nào" là một đường vòng quanh chính cái gốc đó.
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
                "{display} không nằm trong chỉ mục: hoặc ngôn ngữ của nó chưa được hỗ \
                 trợ, hoặc `.gitignore` loại nó ra. Đọc thẳng bằng `read`."
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
