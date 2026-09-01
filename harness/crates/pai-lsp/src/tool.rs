//! `lsp` — một tool, bốn thao tác.
//!
//! Một tool chứ không phải bốn, vì bốn thao tác này chia chung **toàn bộ** tham số và
//! toàn bộ chi phí giải thích: mô hình phải học đúng một lần rằng có một máy hiểu mã đang
//! chạy và nó nhận một con trỏ. Bốn tool riêng là bốn mô tả gần giống nhau trong mỗi
//! request, và bốn cơ hội để mô hình chọn nhầm cái na ná.
//!
//! Về hình dạng kết quả: mọi dòng bắt đầu bằng `đường:dòng:cột`, đúng cái mà `grep`,
//! `symbol_search` và `outline` đã phát ra. Mô hình đã biết đọc hình dạng đó và biết bước
//! kế tiếp là `read`; phát minh một hình dạng thứ hai chỉ để đẹp hơn là bắt nó học lại.

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

    /// Chỉ đọc — không thao tác nào ở đây ghi gì. Không đáng tin — thứ trả về là mã nguồn,
    /// tên biến và thông báo lỗi **do người khác viết**, và một repo bất kỳ chứa được một
    /// dòng comment giả dạng chỉ dẫn. Cùng lý lẽ với `pai-index::tools`.
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
            // Mọi lỗi thành **văn bản đọc được**, không thành `Err`: một lỗi lọt lên đường
            // ống chỉ kết thúc lượt trong im lặng, còn ở đây mỗi nhánh nói ra được việc
            // tiếp theo nên làm — chờ, đổi tool, hay sửa tham số.
            Err(err @ (LspError::Invalid(_) | LspError::NoServer(_))) => {
                Err(ToolError::Invalid(err.to_string()))
            }
            Err(err) => Ok(ToolOutcome::error(err.to_string())),
        }
    }
}

/// Lời nhắc rằng câu trả lời có thể còn thiếu.
///
/// Nó đi kèm **mọi** dạng kết quả khi server còn bận, kể cả kết quả không rỗng: một danh
/// sách tham chiếu thu được giữa lúc đang nạp là một danh sách thiếu, và mô hình đọc nó
/// như một danh sách đủ nếu không ai nói gì.
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

            // Gom theo tệp, cùng hình dạng `meta.search` mà `grep`, `glob` và
            // `symbol_search` phát ra — giao diện vẽ được bằng đúng cái thẻ đã có.
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
