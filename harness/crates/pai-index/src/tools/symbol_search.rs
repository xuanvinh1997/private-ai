//! `symbol_search` — tìm ký hiệu theo tên.
//!
//! Tool này thay thế cái vòng "grep một cái tên, lọc ra khỏi hàng trăm chỗ dùng, đoán xem
//! chỗ nào là chỗ khai báo". Chỉ mục biết chỗ nào là khai báo, nên nó trả về đúng chỗ đó
//! kèm số dòng — và bước sau của mô hình là một lần `read` chính xác thay vì ba lần
//! `grep` nữa.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::index::SymbolIndex;
use crate::symbol::SymbolKind;
use crate::tools::render;

/// Bao nhiêu ký hiệu khi mô hình không nói gì.
const DEFAULT_LIMIT: usize = 30;
/// Trần cứng. Mô hình xin một nghìn kết quả là mô hình đang dùng sai tool, và đưa đủ một
/// nghìn cho nó chỉ làm cửa sổ ngữ cảnh đầy trước khi nó kịp nhận ra.
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

        // Đồng bộ trước mỗi lần hỏi. Với một cây không đổi đây là một loạt `stat` và
        // không có lần parse nào — xem `index::scan`. Cái giá đó rẻ hơn nhiều so với thứ
        // nó mua: mô hình không bao giờ đọc được một chỉ mục nói về mã của mười phút trước.
        let report = self
            .index
            .sync()
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        tracing::debug!(?report, "đồng bộ chỉ mục trước khi tìm ký hiệu");

        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let hits = self
            .index
            .search(&args.query, args.kind, limit)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        if hits.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có ký hiệu nào tên giống `{}`{}. Chỉ mục chỉ chứa Rust, \
                 TypeScript, JavaScript và Python, và bỏ qua những gì `.gitignore` loại \
                 trừ; `grep` tìm được ở chỗ chỉ mục không với tới.",
                args.query,
                match args.kind {
                    Some(kind) => format!(" thuộc loại `{}`", kind.as_str()),
                    None => String::new(),
                }
            )));
        }

        let rendered = hits.iter().map(render).collect::<Vec<_>>().join("\n");

        // Gom theo tệp: mười ký hiệu trong một tệp đọc dễ hơn mười dòng rời rạc lặp lại
        // cùng một đường dẫn. Cùng hình dạng `meta.search` mà `grep` và `glob` phát ra,
        // nên giao diện vẽ được bằng đúng cái thẻ đã có.
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
            // Cắt ở đây là cắt bởi `limit`, và mô hình biết mình đã xin bao nhiêu.
            "truncated": hits.len() == limit,
            "total": hits.len(),
            "groups": groups,
        });
        Ok(ToolOutcome::ok(rendered).with_meta("search", meta))
    }
}
