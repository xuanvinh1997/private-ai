//! `todo_write` — the reference tool, and a real one.
//! The simplest shape a tool can take: schema from a Rust type, session state, no disk or
//! network. The list lives in the tool, not the journal, because it is the model's scratchpad.

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

/// One session's task list.
#[derive(Default)]
pub struct TodoWrite {
    todos: Mutex<Vec<TodoItem>>,
}

impl TodoWrite {
    pub const NAME: &'static str = "todo_write";

    pub fn new() -> TodoWrite {
        TodoWrite::default()
    }

    /// A snapshot for the UI.
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
        // Not `mutating`: it only writes this turn's scratchpad, so a read-only agent may still plan.
        // Not concurrency-safe: each write replaces the whole list, so one of two parallel writes vanishes.
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
