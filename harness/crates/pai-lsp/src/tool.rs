//! `lsp` - one tool, four operations.
//! One tool because the four share every argument and every explanation; four would be
//! four near-identical descriptions to confuse. Output lines start with `path:line:col`.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::error::LspError;
use crate::seam::{Answer, LanguageServers, Operation, Query};

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolOperation {
    /// Nơi ký hiệu dưới con trỏ được khai báo, kể cả ở tệp khác hay ở một crate khác.
    Definition,
    /// Mọi nơi tham chiếu tới nó, kèm cả chỗ khai báo.
    References,
    /// Kiểu, chữ ký và tài liệu của nó.
    Hover,
    /// Lỗi và cảnh báo của trình biên dịch cho cả tệp. Không cần con trỏ.
    Diagnostics,
}

impl From<ToolOperation> for Operation {
    fn from(value: ToolOperation) -> Operation {
        match value {
            ToolOperation::Definition => Operation::Definition,
            ToolOperation::References => Operation::References,
            ToolOperation::Hover => Operation::Hover,
            ToolOperation::Diagnostics => Operation::Diagnostics,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LspArgs {
    pub operation: ToolOperation,
    /// Đường dẫn tệp, tuyệt đối hoặc tương đối với thư mục làm việc.
    pub file_path: String,
    /// Dòng của con trỏ, đếm từ 1. Bắt buộc trừ `diagnostics`.
    pub line: Option<u32>,
    /// Cột của con trỏ, đếm từ 1 theo ký tự. Bắt buộc trừ `diagnostics`.
    pub character: Option<u32>,
}

pub struct LspTool {
    servers: Arc<dyn LanguageServers>,
}

impl LspTool {
    pub const NAME: &'static str = "lsp";

    pub fn new(servers: Arc<dyn LanguageServers>) -> LspTool {
        LspTool { servers }
    }
}

#[async_trait]
impl Tool for LspTool {
    fn schema(&self) -> ToolSchema {
        let languages = self.servers.languages().join(", ");
        ToolSchema::new(
            LspTool::NAME,
            format!(
                "Hỏi một language server đang chạy về mã nguồn, tại một con trỏ cụ thể. \
                 Ngôn ngữ có server trên máy này: {languages}.\n\n\
                 Dùng nó cho những gì tìm-theo-chữ không trả lời được: `definition` đi tới \
                 nơi khai báo thật qua `use` và qua nhiều tệp; `references` liệt kê mọi \
                 nơi *tham chiếu* chứ không phải mọi nơi trùng chữ; `hover` cho kiểu suy \
                 ra được và chữ ký; `diagnostics` cho lỗi biên dịch thật của cả tệp.\n\n\
                 Cần biết một cái tên khai ở đâu mà không có sẵn con trỏ thì \
                 `symbol_search` nhanh hơn và không cần server nào. Con trỏ phải trỏ đúng \
                 vào ký hiệu — hãy `read` tệp trước để lấy đúng dòng và cột. Dòng và cột \
                 đếm từ 1, giống hệt số dòng mà `read` in ra."
            ),
            json_schema_for::<LspArgs>(),
        )
    }

    /// Read-only, and untrusted: what comes back is source, identifiers and compiler messages written by other people.
    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: LspArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let op: Operation = args.operation.into();
        if op.needs_position() && (args.line.is_none() || args.character.is_none()) {
            return Err(ToolError::Invalid(format!(
                "`{}` cần `line` và `character` (đếm từ 1) để biết hỏi về ký hiệu nào",
                op.as_str()
            )));
        }

        let query = Query {
            op,
            path: PathBuf::from(&args.file_path),
            line: args.line.unwrap_or(1),
            column: args.character.unwrap_or(1),
        };

        match self.servers.ask(&query).await {
            Ok(answer) => Ok(render(op, &args.file_path, answer)),
            // Every error becomes readable text rather than an `Err`, so each branch can name the next step: wait, switch tool, or fix the arguments.
            Err(err @ (LspError::Invalid(_) | LspError::NoServer(_))) => {
                Err(ToolError::Invalid(err.to_string()))
            }
            Err(err) => Ok(ToolOutcome::error(err.to_string())),
        }
    }
}

/// Notice that the answer may be incomplete; it rides along with every result shape while the server is busy, because a partial reference list otherwise reads as complete.
const STILL_INDEXING: &str = "Language server còn đang nạp và lập chỉ mục dự án, nên kết quả này có thể chưa đầy \
     đủ. Hỏi lại sau vài giây nếu nó trông thiếu.";

fn render(op: Operation, file_path: &str, answer: Answer) -> ToolOutcome {
    match answer {
        Answer::Hover { text, busy } => {
            let body = if text.is_empty() {
                format!("Không có thông tin nào tại con trỏ trong `{file_path}`.")
            } else {
                text
            };
            finish(body, busy, json!({ "shape": "text" }))
        }

        Answer::Diagnostics { notes, busy } => {
            if notes.is_empty() {
                return finish(
                    format!("Không có lỗi hay cảnh báo nào trong `{file_path}`."),
                    busy,
                    json!({ "shape": "matches", "total": 0 }),
                );
            }
            let lines: Vec<String> = notes
                .iter()
                .map(|note| {
                    let source = note
                        .source
                        .as_deref()
                        .map(|s| format!(" [{s}]"))
                        .unwrap_or_default();
                    format!(
                        "{file_path}:{}:{} {}{source} — {}",
                        note.line, note.column, note.severity, note.message
                    )
                })
                .collect();
            let matches: Vec<_> = notes
                .iter()
                .map(|note| json!({ "line": note.line, "text": format!("{} — {}", note.severity, note.message) }))
                .collect();
            finish(
                lines.join("\n"),
                busy,
                json!({
                    "shape": "matches",
                    "total": notes.len(),
                    "truncated": false,
                    "groups": [{ "path": file_path, "matches": matches }],
                }),
            )
        }

        Answer::Locations {
            hits,
            truncated,
            busy,
        } => {
            if hits.is_empty() {
                return finish(
                    format!(
                        "Language server không trả về vị trí nào cho `{}` tại con trỏ đó \
                         trong `{file_path}`. Hãy kiểm lại rằng dòng và cột trỏ đúng vào \
                         một ký hiệu.",
                        op.as_str()
                    ),
                    busy,
                    json!({ "shape": "matches", "total": 0 }),
                );
            }
            let lines: Vec<String> = hits
                .iter()
                .map(|hit| {
                    let mark = if hit.reachable {
                        String::new()
                    } else {
                        " (ngoài thư mục làm việc; `read` không mở được)".to_string()
                    };
                    let text = if hit.text.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", hit.text)
                    };
                    format!("{}:{}:{}{mark}{text}", hit.path, hit.line, hit.column)
                })
                .collect();

            // Grouped by file, the same `meta.search` shape `grep`, `glob` and `symbol_search` emit, so the UI reuses its existing card.
            let mut groups: Vec<serde_json::Value> = Vec::new();
            for hit in &hits {
                let entry = json!({ "line": hit.line, "text": hit.text });
                match groups.last_mut() {
                    Some(group) if group["path"] == hit.path.as_str() => {
                        if let Some(list) = group["matches"].as_array_mut() {
                            list.push(entry);
                        }
                    }
                    _ => groups.push(json!({ "path": hit.path, "matches": [entry] })),
                }
            }

            let mut body = lines.join("\n");
            if truncated {
                body.push_str("\n… còn nữa; chỉ hiện những vị trí đầu tiên.");
            }
            finish(
                body,
                busy,
                json!({
                    "shape": "matches",
                    "total": hits.len(),
                    "truncated": truncated,
                    "groups": groups,
                }),
            )
        }
    }
}

fn finish(mut body: String, busy: bool, meta: serde_json::Value) -> ToolOutcome {
    if busy {
        body.push_str("\n\n");
        body.push_str(STILL_INDEXING);
    }
    ToolOutcome::ok(body).with_meta("search", meta)
}
