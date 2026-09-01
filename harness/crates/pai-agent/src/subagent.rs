//! Giao việc cho một agent con.
//!
//! Một subagent là một lượt trọn vẹn chạy trong **phiên riêng** và **phạm vi riêng**. Hai
//! chữ "riêng" đó là toàn bộ lý do nó tồn tại:
//!
//! **Phiên riêng** nghĩa là mọi thứ nó đọc — hai mươi tệp, năm lần `grep`, một cây thư
//! mục — nằm trong sổ của nó, không nằm trong ngữ cảnh của agent cha. Cha chỉ nhận lại
//! bản báo cáo. Đó là cách một việc tốn năm mươi nghìn token trả về một đoạn văn năm dòng.
//!
//! **Phạm vi riêng** nghĩa là bộ tool của con hẹp lại được mà không đụng tới cha. Cụ thể
//! và bắt buộc: con **không được** giao việc tiếp khi đã tới đáy. Không có trần đó thì
//! một mô hình đang bí sẽ giao việc cho chính hình ảnh của nó, mãi mãi, và thứ chặn lại
//! là hết tiền chứ không phải một dòng mã.
//!
//! Con thừa hưởng prompt hệ thống và bộ tool của cha, chứ không phải một bản rút gọn nào
//! khác. Cho con một thế giới khác là tạo ra một lớp hành vi thứ hai phải nuôi song song,
//! và người đọc báo cáo sẽ không biết con đã nhìn thấy gì.

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

/// Sâu bao nhiêu thì thôi.
///
/// Hai tầng là đủ cho việc thật (cha giao, con làm) và đủ ngắn để một vòng lặp vô tình
/// dừng lại trước khi ai kịp nhận ra. Nới nó ra là quyết định phải có lý do.
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

/// Chạy agent con ngay trong tiến trình này.
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

        // Phạm vi riêng. Mọi hạn chế đặt ở đây chỉ chạm tới con, không chạm tới cha.
        let child = self.ctx.scoped("subagent");
        let registry = self.ctx.require::<Tools>().map_err(|err| err.to_string())?;
        let scope = child.scope_key().ok_or("phạm vi con không dựng được")?;

        // Ở tầng cuối, `task` bị rút khỏi bộ tool của con — chứ không phải để nó gọi rồi
        // nhận một lời từ chối. Một tool nhìn thấy được mà lần nào cũng hỏng là một tool
        // dạy mô hình bỏ qua danh sách.
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

        // Báo cáo là **lời cuối cùng của con**, không phải cả bản ghi. Cả bản ghi vẫn nằm
        // trong sổ của con, đọc lại được — nhưng đưa nó lên cho cha là xoá sạch cái lợi
        // duy nhất của việc giao việc.
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
        // Con làm được gì thì đây làm được nấy, kể cả sửa tệp — nên nó không phải chỉ-đọc.
        // Nội dung trả về là lời một mô hình viết sau khi đọc tệp của người dùng, nên nó
        // cũng không đáng tin hơn chính những tệp đó.
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

/// Cắm việc giao việc vào cây.
///
/// Nó cần cả mô hình lẫn sổ phiên, tức là hai thứ mà phần lớn plugin không cần. Đó là
/// điều đúng chứ không phải một chỗ rò rỉ: một agent con **là** một lượt trọn vẹn, nên
/// nó cần đúng những gì một lượt cần.
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
        // Agent gốc ở độ sâu 0. Con của nó tự đếm tiếp khi `delegate` dựng phạm vi mới.
        ctx.keep(registry.register(Arc::new(Task::new(provider, 0))));
        Ok(())
    }
}
