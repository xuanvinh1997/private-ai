//! `symbol_search` — find symbols by name.
//! Replaces the grep-then-guess-the-declaration loop: the index knows which site is the
//! declaration, so the model's next step is one exact `read` instead of three more greps.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::index::SymbolIndex;
use crate::symbol::SymbolKind;
use crate::tools::render;

/// How many symbols when the model says nothing.
const DEFAULT_LIMIT: usize = 30;
/// Hard ceiling: asking for a thousand results is misuse, and delivering them fills the context first.
const MAX_LIMIT: usize = 200;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolSearchArgs {
    /// Tên ký hiệu, hoặc một phần đầu của tên.
    pub query: String,
    /// Chỉ lấy một loại ký hiệu.
    pub kind: Option<SymbolKind>,
    /// Tối đa bao nhiêu kết quả. Mặc định 30, trần 200.
    pub limit: Option<usize>,
}

pub struct SymbolSearch {
    index: Arc<dyn SymbolIndex>,
}

impl SymbolSearch {
    pub const NAME: &'static str = "symbol_search";

    pub fn new(index: Arc<dyn SymbolIndex>) -> SymbolSearch {
        SymbolSearch { index }
    }
}

#[async_trait]
impl Tool for SymbolSearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            SymbolSearch::NAME,
            "Tìm nơi **khai báo** một hàm, kiểu, trait hay hằng trong thư mục làm việc, \
             theo tên. Trả về đường dẫn và số dòng để đọc tiếp bằng `read`. Chỉ mục là \
             cú pháp thuần tuý (Rust, TypeScript, JavaScript, Python) nên nó tìm theo \
             tên, không theo ý nghĩa: hỏi `resolve_read` được, hỏi `chỗ kiểm tra đường \
             dẫn` thì dùng `grep`. Khớp đúng tên xếp trước; sau chúng có thể là những \
             khai báo chỉ nhắc tới tên đó trong dòng chữ ký.",
            json_schema_for::<SymbolSearchArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: SymbolSearchArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        // Sync before every query; on an unchanged tree this is only `stat` calls — see `index::scan`.
        let report = self
            .index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        tracing::debug!(?report, "synced the index before the symbol search");

        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let hits = self
            .index
            .search(&args.query, args.kind, limit)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if hits.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có ký hiệu nào tên giống `{}`{}. Chỉ mục chỉ chứa Rust, \
                 TypeScript, JavaScript và Python, có trần kích thước/số tệp, và bỏ qua \
                 mã sinh cùng những gì `.gitignore` loại trừ; `grep` tìm được ở chỗ chỉ \
                 mục không với tới.",
                args.query,
                match args.kind {
                    Some(kind) => format!(" thuộc loại `{}`", kind.as_str()),
                    None => String::new(),
                }
            )));
        }

        let rendered = hits.iter().map(render).collect::<Vec<_>>().join("\n");

        // Group by file, in the same `meta.search` shape `grep` and `glob` emit, so the UI reuses one card.
        let mut groups: Vec<serde_json::Value> = Vec::new();
        for hit in &hits {
            let entry = json!({
                "line": hit.start_line,
                "text": format!("{} {} — {}", hit.kind.as_str(), hit.qualified(), hit.signature),
            });
            match groups.last_mut() {
                Some(group) if group["path"] == hit.path.as_str() => {
                    if let Some(list) = group["matches"].as_array_mut() {
                        list.push(entry);
                    }
                }
                _ => groups.push(json!({ "path": hit.path, "matches": [entry] })),
            }
        }

        let meta = json!({
            "shape": "matches",
            // A cut here is the `limit`, and the model knows what it asked for.
            "truncated": hits.len() == limit,
            "total": hits.len(),
            "groups": groups,
        });
        Ok(ToolOutcome::ok(rendered).with_meta("search", meta))
    }
}
