//! `todo_write` — tool mẫu, và cũng là tool thật.
//!
//! Nó ở đây vì nó là hình dạng đơn giản nhất mà một tool có thể có: schema sinh từ một
//! kiểu Rust, trạng thái thuộc về phiên, không đụng đĩa, không đụng mạng. Ai viết tool
//! tiếp theo chép cái này.
//!
//! Danh sách sống trong tool chứ không trong sổ phiên, và đó là chủ ý: nó là bản nháp của
//! mô hình, không phải nguồn ngữ cảnh. Cái vào sổ là `tools/result` — thứ mô hình đọc
//! lại được ở lượt sau.

use async_trait::async_trait;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::schema::{ToolMeta, ToolSchema, json_schema_for};
use crate::tool::{Invocation, Tool, ToolError, ToolOutcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TodoItem {
    /// Việc cần làm, viết ở dạng mệnh lệnh.
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TodoWriteArgs {
    /// Toàn bộ danh sách. Mỗi lần ghi là ghi đè, không phải ghi thêm.
    pub todos: Vec<TodoItem>,
}

/// Danh sách việc của một phiên.
#[derive(Default)]
pub struct TodoWrite {
    todos: Mutex<Vec<TodoItem>>,
}

impl TodoWrite {
    pub const NAME: &'static str = "todo_write";

    pub fn new() -> TodoWrite {
        TodoWrite::default()
    }

    /// Ảnh chụp cho giao diện.
    pub fn snapshot(&self) -> Vec<TodoItem> {
        self.todos.lock().clone()
    }

    fn render(todos: &[TodoItem]) -> String {
        if todos.is_empty() {
            return "Danh sách việc trống.".to_string();
        }
        todos
            .iter()
            .map(|item| {
                let mark = match item.status {
                    TodoStatus::Pending => " ",
                    TodoStatus::InProgress => "~",
                    TodoStatus::Completed => "x",
                };
                format!("- [{mark}] {}", item.content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl Tool for TodoWrite {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TodoWrite::NAME,
            "Ghi lại danh sách việc của lượt hiện tại. Gửi toàn bộ danh sách mỗi lần: \
             lần ghi sau thay thế lần ghi trước. Đúng một việc được ở trạng thái \
             `in_progress`.",
            json_schema_for::<TodoWriteArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Không phải `mutating`: nó chỉ ghi vào bản nháp của chính lượt này. Không có tệp
        // nào, bản ghi nào hay thiết lập nào ở ngoài phiên thay đổi vì nó, nên một agent
        // chỉ-đọc vẫn phải được lập kế hoạch.
        //
        // `concurrency_safe = false`: mỗi lần ghi thay cả danh sách, nên hai lần ghi song
        // song thì một trong hai biến mất mà không ai biết.
        ToolMeta::read_only().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: TodoWriteArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let running = args
            .todos
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        if running > 1 {
            return Err(ToolError::Invalid(format!(
                "{running} việc đang ở `in_progress`; chỉ được một."
            )));
        }

        let mut state = self.todos.lock();
        *state = args.todos;
        let rendered = TodoWrite::render(&state);
        let structured = serde_json::to_value(&*state).unwrap_or(json!([]));
        drop(state);

        Ok(ToolOutcome::ok(rendered).with_structured(structured))
    }
}
