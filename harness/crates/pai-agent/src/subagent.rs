//! Delegating work to a subagent: a whole turn in its own session and its own scope.
//! The child's reading stays in its journal and the parent gets only the report; the child's
//! tool set narrows without touching the parent, and depth is capped so recursion ends.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, ServiceKey};
use pai_llm::LlmAdapter;
use pai_session::{Message, NewSession, Origin, Role, SessionService};
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolPipeline, ToolRestriction, ToolSchema,
    Tools, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::driver::{Driver, Silent};
use crate::prompt::Prompt;

/// How deep before stopping; two levels suffice for real work and stop an accidental loop early.
pub const MAX_DEPTH: u32 = 2;

#[derive(Debug, Clone)]
pub struct SubagentReport {
    pub session_id: String,
    pub text: String,
    pub steps: u64,
}

#[async_trait]
pub trait SubagentProvider: Send + Sync + 'static {
    async fn delegate(&self, prompt: &str, depth: u32) -> Result<SubagentReport, String>;
}

pub enum Subagents {}
impl ServiceKey for Subagents {
    type Api = dyn SubagentProvider;
    const NAME: &'static str = "subagents";
}

/// Run subagents inside this process.
pub struct LocalSubagents {
    ctx: Context,
    llm: Arc<dyn LlmAdapter>,
    sessions: SessionService,
    model: String,
    cwd: String,
}

impl LocalSubagents {
    pub fn new(
        ctx: Context,
        llm: Arc<dyn LlmAdapter>,
        sessions: SessionService,
        model: impl Into<String>,
        cwd: impl Into<String>,
    ) -> LocalSubagents {
        LocalSubagents {
            ctx,
            llm,
            sessions,
            model: model.into(),
            cwd: cwd.into(),
        }
    }
}

#[async_trait]
impl SubagentProvider for LocalSubagents {
    async fn delegate(&self, prompt: &str, depth: u32) -> Result<SubagentReport, String> {
        if depth >= MAX_DEPTH {
            return Err(format!(
                "đã tới đáy {MAX_DEPTH} tầng giao việc; hãy tự làm phần còn lại thay vì \
                 giao tiếp."
            ));
        }

        // Its own scope: restrictions set here reach the child only, never the parent.
        let child = self.ctx.scoped("subagent");
        let registry = self.ctx.require::<Tools>().map_err(|err| err.to_string())?;
        let scope = child.scope_key().ok_or("phạm vi con không dựng được")?;

        // At the last level `task` is removed rather than left to refuse: a visible tool that always fails teaches neglect.
        let restriction = ToolRestriction {
            allow: None,
            deny: if depth + 1 >= MAX_DEPTH {
                [Task::NAME.into()].into_iter().collect()
            } else {
                Default::default()
            },
        };
        child.keep(registry.restrict(scope, restriction));

        let pipeline = Arc::new(ToolPipeline::new(&child, registry));
        let system = self
            .ctx
            .require::<Prompt>()
            .map_err(|err| err.to_string())?;
        let driver = Driver::new(
            child.clone(),
            self.llm.clone(),
            pipeline,
            system,
            self.model.clone(),
        );

        let mut session = self
            .sessions
            .create(NewSession {
                cwd: Some(self.cwd.clone()),
                origin: Some(Origin::Subagent),
                delegation_depth: Some(depth + 1),
                ..NewSession::default()
            })
            .await
            .map_err(|err| err.to_string())?;
        let session_id = session.id().to_string();

        driver
            .run_turn(
                &mut session,
                1,
                vec![Message::user(prompt)],
                child.effects().cancel_token(),
                &Silent,
            )
            .await
            .map_err(|err| err.to_string())?;

        // The report is the child's last word, not its whole record, which would erase the only benefit of delegating.
        let history = session.derive_messages();
        let text = history
            .iter()
            .rev()
            .find(|message| message.role == Role::Assistant)
            .map(|message| {
                message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        pai_session::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        Ok(SubagentReport {
            session_id,
            steps: history.len() as u64,
            text,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskArgs {
    /// Việc cần làm, viết đủ để một agent không có ngữ cảnh của bạn làm được.
    pub prompt: String,
}

pub struct Task {
    provider: Arc<dyn SubagentProvider>,
    depth: u32,
}

impl Task {
    pub const NAME: &'static str = "task";

    pub fn new(provider: Arc<dyn SubagentProvider>, depth: u32) -> Task {
        Task { provider, depth }
    }
}

#[async_trait]
impl Tool for Task {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Task::NAME,
            "Giao một việc con cho một agent riêng và nhận lại bản tóm tắt. Dùng khi việc \
             cần đọc nhiều tệp nhưng kết quả thì ngắn — agent con đọc trong ngữ cảnh của \
             nó, bạn chỉ nhận về câu trả lời. Viết đề bài đủ để một người không biết gì \
             về cuộc trò chuyện này vẫn làm được.",
            json_schema_for::<TaskArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // This does whatever the child does, edits included, and returns model text about user files: not read-only, not trusted.
        ToolMeta::mutating().untrusted().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: TaskArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        match self.provider.delegate(&args.prompt, self.depth).await {
            Ok(report) => Ok(
                ToolOutcome::ok(report.text).with_structured(serde_json::json!({
                    "session_id": report.session_id,
                    "steps": report.steps,
                })),
            ),
            Err(reason) => Err(ToolError::Failed(reason)),
        }
    }
}

/// Mount delegation into the tree; it needs the model and the session store because a subagent is a whole turn.
pub struct SubagentPlugin {
    llm: Arc<dyn LlmAdapter>,
    sessions: SessionService,
    model: String,
    cwd: String,
}

impl SubagentPlugin {
    pub fn new(
        llm: Arc<dyn LlmAdapter>,
        sessions: SessionService,
        model: impl Into<String>,
        cwd: impl Into<String>,
    ) -> SubagentPlugin {
        SubagentPlugin {
            llm,
            sessions,
            model: model.into(),
            cwd: cwd.into(),
        }
    }
}

#[async_trait]
impl pai_core::Plugin for SubagentPlugin {
    fn name(&self) -> &'static str {
        "subagent"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let provider: Arc<dyn SubagentProvider> = Arc::new(LocalSubagents::new(
            ctx.clone(),
            self.llm.clone(),
            self.sessions.clone(),
            self.model.clone(),
            self.cwd.clone(),
        ));
        ctx.keep(ctx.provide::<Subagents>(provider.clone())?);

        let registry = ctx.require::<Tools>()?;
        // The root agent is depth 0; children count up as `delegate` builds each new scope.
        ctx.keep(registry.register(Arc::new(Task::new(provider, 0))));
        Ok(())
    }
}
