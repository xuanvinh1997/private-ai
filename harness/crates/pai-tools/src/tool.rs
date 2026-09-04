//! A call's vocabulary: the request, the result, and the trait every tool implements.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::name::ToolName;
use crate::schema::{ToolMeta, ToolSchema};

/// What a tool body may return; folded into a [`ToolOutcome`] at the outer edge, since a leaked `Result` ends the turn silently.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Unusable arguments; the model can fix these, so tell it what to fix.
    #[error("tham số không hợp lệ: {0}")]
    Invalid(String),
    /// The tool body ran and failed.
    #[error("{0}")]
    Failed(String),
    /// The user was asked and said no.
    #[error("người dùng từ chối: {0}")]
    Refused(String),
}

/// One call's result, after every layer has run.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolOutcome {
    /// The text the model reads.
    pub content: String,
    /// The tool's typed return value, if any; this is what the UI should draw.
    pub structured: Option<Value>,
    pub is_error: bool,
    /// Host metadata such as diffs, spill locators and refusal reasons; never sent to the model.
    pub meta: Map<String, Value>,
}

impl ToolOutcome {
    pub fn ok(content: impl Into<String>) -> ToolOutcome {
        ToolOutcome {
            content: content.into(),
            structured: None,
            is_error: false,
            meta: Map::new(),
        }
    }

    /// A failure is still a readable result, not an exception.
    pub fn error(content: impl Into<String>) -> ToolOutcome {
        ToolOutcome {
            is_error: true,
            ..ToolOutcome::ok(content)
        }
    }

    pub fn with_structured(mut self, value: Value) -> ToolOutcome {
        self.structured = Some(value);
        self
    }

    pub fn with_meta(mut self, key: impl Into<String>, value: Value) -> ToolOutcome {
        self.meta.insert(key.into(), value);
        self
    }
}

/// Ask the user for a value matching a JSON Schema; unlike an approver this asks for data, not permission.
#[async_trait]
pub trait Elicitor: Send + Sync + 'static {
    /// `None` means no answer arrived: cancelled, timed out, or no UI at all.
    async fn elicit(&self, prompt: &str, schema: &Value) -> Option<Value>;
}

/// A call in flight; the arguments here are post-pinning, so a tool reads what the host decided.
pub struct Invocation {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    elicitor: Option<Arc<dyn Elicitor>>,
    /// Cancelled on timeout; a tool body should watch it rather than run on after its result is dropped.
    cancel: CancellationToken,
}

impl Invocation {
    pub fn new(
        name: ToolName,
        call_id: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> Invocation {
        Invocation {
            name,
            call_id: call_id.into(),
            arguments,
            elicitor: None,
            cancel: CancellationToken::new(),
        }
    }

    pub fn with_elicitor(mut self, elicitor: Option<Arc<dyn Elicitor>>) -> Invocation {
        self.elicitor = elicitor;
        self
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn arg(&self, field: &str) -> Option<&Value> {
        self.arguments.get(field)
    }

    pub fn str_arg(&self, field: &str) -> Option<&str> {
        self.arguments.get(field).and_then(Value::as_str)
    }

    /// Ask the user for a value; with no UI mounted this returns `None`, fail-closed as approval is.
    pub async fn elicit(&self, prompt: &str, schema: &Value) -> Option<Value> {
        let elicitor = self.elicitor.clone()?;
        elicitor.elicit(prompt, schema).await
    }

    /// An argument snapshot for the journal and the UI.
    pub fn snapshot(&self) -> Value {
        json!({ "name": self.name.as_str(), "call_id": self.call_id, "arguments": self.arguments })
    }
}

/// A tool; object-safe, because the registry holds `Arc<dyn Tool>` and an MCP tool has no static type.
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// What the model sees; the registry still reframes the description and hides pinned parameters.
    fn schema(&self) -> ToolSchema;

    /// What only the host sees; the worst-case assumption by default — see [`ToolMeta::default`].
    fn meta(&self) -> ToolMeta {
        ToolMeta::default()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError>;

    /// The last hook: synchronous so it cannot ask anyone anything, and content-only so it cannot flip `is_error`.
    fn finalize(&self, _content: &mut String) {}
}
