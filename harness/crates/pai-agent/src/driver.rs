//! Vòng lặp turn/step.
//!
//! Viết tay bằng `loop`, không phải một máy trạng thái đồ thị. Trình tự ở đây ngắn đến
//! mức đọc hết trong một màn hình, và đó là điểm mạnh chứ không phải sự thô sơ: mọi thứ
//! phức tạp — phê duyệt, sandbox, hook, nén ngữ cảnh — cắm vào từ ngoài qua bốn điểm nối
//! trong [`crate::events`], nên vòng lặp không phải lớn lên theo số tính năng.
//!
//! Hai luật giữ nguyên từ bản Python, cả hai đều là chỗ đã từng sai:
//!
//! **Lịch sử mô hình dựng từ sổ, không từ một bản sao trong bộ nhớ.** Có hai nguồn sự
//! thật thì sớm muộn chúng lệch nhau, và cái lệch chỉ lộ ra khi mở lại phiên cũ.
//!
//! **Vòng cuối cùng không được trao tool.** Chỉ có trần số vòng thì lượt kết thúc bằng
//! một lời gọi tool không ai trả lời — mô hình treo giữa câu, và bản ghi không giải thích
//! được vì sao.

use std::sync::Arc;

use futures::FutureExt;
use futures::StreamExt;
use pai_core::Context;
use pai_llm::{ChatRequest, LlmAdapter, Message as LlmMessage, StreamChunk};
use pai_session::{
    AssistantChunk, AssistantMessage, ContentBlock as LogBlock, Message as LogMessage, Role,
    Session, SessionEvent, StepEnd, StepStart, ToolCall as LogToolCall,
    ToolResult as LogToolResult, TurnEnd, TurnEndReason, TurnStart,
};
use pai_tools::ToolPipeline;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::bridge::{assistant_to_log, to_llm_history};
use crate::events::{AgentRequest, PreStep, PreStepRequest, StepDecision, TurnStop, TurnStopping};
use crate::prompt::SystemPrompt;

/// Nơi vòng lặp kể lại những gì đang xảy ra, theo thời gian thực.
///
/// Tách khỏi sổ vì hai thứ trả lời hai câu hỏi khác nhau: sổ trả lời "mô hình đã thấy
/// gì", sink trả lời "người dùng đang nhìn thấy gì". Trộn chúng lại là cách một chi tiết
/// hiển thị lọt vào ngữ cảnh của mô hình.
pub trait TurnSink: Send + Sync {
    fn token(&self, _text: &str) {}
    fn tool_start(&self, _call_id: &str, _name: &str, _arguments: &str) {}
    /// `meta` là phần dành riêng cho giao diện — diff, danh sách khớp, output terminal.
    /// Mô hình **không** thấy nó; nó đi song song với `content` chứ không nằm trong đó,
    /// nên một khối diff hiện ra cho người dùng không tốn một token nào của mô hình.
    fn tool_end(
        &self,
        _call_id: &str,
        _name: &str,
        _is_error: bool,
        _preview: &str,
        _meta: &Map<String, Value>,
    ) {
    }
    fn notice(&self, _message: &str) {}
}

/// Sink không làm gì. Dùng cho chạy không giao diện và cho test.
pub struct Silent;
impl TurnSink for Silent {}

pub struct Driver {
    ctx: Context,
    llm: Arc<dyn LlmAdapter>,
    tools: Arc<ToolPipeline>,
    prompt: Arc<SystemPrompt>,
    model: String,
    /// Trần số bước trong một lượt.
    max_steps: u64,
}

impl Driver {
    pub fn new(
        ctx: Context,
        llm: Arc<dyn LlmAdapter>,
        tools: Arc<ToolPipeline>,
        prompt: Arc<SystemPrompt>,
        model: impl Into<String>,
    ) -> Driver {
        Driver {
            ctx,
            llm,
            tools,
            prompt,
            model: model.into(),
            max_steps: 12,
        }
    }

    pub fn with_max_steps(mut self, steps: u64) -> Driver {
        self.max_steps = steps.max(1);
        self
    }

    /// Chạy một lượt tới khi không còn gì nợ.
    pub async fn run_turn(
        &self,
        session: &mut Session,
        turn: u64,
        input: Vec<LogMessage>,
        cancel: CancellationToken,
        sink: &dyn TurnSink,
    ) -> anyhow::Result<TurnEndReason> {
        session
            .append(SessionEvent::TurnStart(TurnStart { turn }))
            .await?;
        let reason = self.drive(session, turn, input, cancel, sink).await;
        let reason = match reason {
            Ok(reason) => reason,
            Err(err) => TurnEndReason::Error {
                message: err.to_string(),
            },
        };
        session
            .append(SessionEvent::TurnEnd(TurnEnd {
                turn,
                reason: reason.clone(),
            }))
            .await?;
        session.flush().await?;
        Ok(reason)
    }

    async fn drive(
        &self,
        session: &mut Session,
        turn: u64,
        input: Vec<LogMessage>,
        cancel: CancellationToken,
        sink: &dyn TurnSink,
    ) -> anyhow::Result<TurnEndReason> {
        let mut pending = input;
        let mut step = 0u64;

        loop {
            step += 1;

            let mut request = PreStepRequest {
                turn,
                step,
                messages: std::mem::take(&mut pending),
                history: session.derive_messages(),
            };
            let decision = self
                .ctx
                .waterfall::<PreStep, _>(&mut request, |req| {
                    let messages = req.messages.clone();
                    async move { StepDecision::enter(messages) }.boxed()
                })
                .await;

            let entered = match decision {
                StepDecision::Enter { messages, replace } => {
                    // Che trước khi message mới vào sổ: vị trí trong `replace` tính trên
                    // phép chiếu mà listener vừa nhìn thấy, và thêm gì vào trước sẽ làm
                    // mọi vị trí đó trượt đi một chỗ.
                    if let Some(replace) = replace {
                        session
                            .append_replacing(
                                SessionEvent::UserMessage(replace.summary),
                                replace.start,
                                replace.end,
                            )
                            .await?;
                    }
                    messages
                }
                StepDecision::Reject { reason } => {
                    // Lượt vẫn đóng lại bình thường và không tiêu bước nào. Bản ghi phải
                    // nhớ là đã có người thử, kể cả khi chẳng có gì được gửi đi.
                    sink.notice(&reason);
                    return Ok(TurnEndReason::Completed);
                }
            };

            session
                .append(SessionEvent::StepStart(StepStart { turn, step }))
                .await?;
            for message in entered {
                session
                    .append_surface(SessionEvent::UserMessage(message))
                    .await?;
            }

            let last_round = step >= self.max_steps;
            let assistant = self
                .one_step(session, turn, step, last_round, &cancel, sink)
                .await?;
            let calls = assistant.tool_calls();

            for call in &calls {
                self.run_tool(session, turn, step, call, sink).await?;
            }

            session
                .append(SessionEvent::StepEnd(StepEnd { turn, step }))
                .await?;

            if cancel.is_cancelled() {
                return Ok(TurnEndReason::Interrupted);
            }
            if !calls.is_empty() {
                if last_round {
                    // Không xảy ra được: vòng cuối không có schema tool nào để mà gọi.
                    return Ok(TurnEndReason::MaxSteps);
                }
                continue;
            }

            // Không còn nợ tool. Hỏi xem có ai muốn nối thêm việc không.
            match self.ctx.first::<TurnStop>(&TurnStopping { turn }).await {
                Some(more) if !more.is_empty() => pending = more,
                _ => return Ok(TurnEndReason::Completed),
            }
            if step >= self.max_steps {
                return Ok(TurnEndReason::MaxSteps);
            }
        }
    }

    /// Một lần gọi mô hình, từ lúc dựng request tới lúc message vào sổ.
    async fn one_step(
        &self,
        session: &mut Session,
        turn: u64,
        step: u64,
        last_round: bool,
        cancel: &CancellationToken,
        sink: &dyn TurnSink,
    ) -> anyhow::Result<LlmMessage> {
        let history = to_llm_history(&session.derive_messages());
        let mut messages = Vec::with_capacity(history.len() + 1);
        let system = self.prompt.assemble();
        if !system.is_empty() {
            messages.push(LlmMessage::system(system));
        }
        messages.extend(history);

        let tools = if last_round {
            Vec::new()
        } else {
            self.tools
                .registry()
                .schemas(self.ctx.scope_key())
                .into_iter()
                .map(|schema| {
                    // Mô hình thấy tên đã mã hoá; sổ đăng ký nhận lại cả hai dạng.
                    pai_llm::ToolSchema::new(
                        schema.name.wire(),
                        schema.description,
                        schema.parameters,
                    )
                })
                .collect()
        };

        let mut request = ChatRequest::new(&self.model)
            .with_messages(messages)
            .with_tools(tools);
        let request = self
            .ctx
            .waterfall::<AgentRequest, _>(&mut request, |req| {
                let cloned = req.clone();
                async move { cloned }.boxed()
            })
            .await;

        let mut assembler = pai_llm::BlockAssembler::new();
        let mut stream = self.llm.stream(request);
        let mut interrupted = false;
        let mut failure = None;

        loop {
            tokio::select! {
                // Huỷ không nhảy ra khỏi hàm: phần trả lời dở vẫn phải vào sổ ở dưới.
                // Thoát sớm ở đây là đúng cái lỗi khiến người dùng mất nửa câu trả lời.
                _ = cancel.cancelled() => { interrupted = true; break }
                next = stream.next() => match next {
                    None => break,
                    Some(Err(err)) => { failure = Some(err.to_string()); break }
                    Some(Ok(chunk)) => {
                        if let StreamChunk::TextDelta { text, .. } = &chunk {
                            sink.token(text);
                        }
                        if let Ok(value) = serde_json::to_value(&chunk) {
                            session
                                .append(SessionEvent::AssistantChunk(AssistantChunk {
                                    turn,
                                    step,
                                    chunk: value,
                                }))
                                .await?;
                        }
                        assembler.push(&chunk);
                    }
                }
            }
        }
        drop(stream);

        let message = assembler.message();
        session
            .append_surface(SessionEvent::AssistantMessage(AssistantMessage {
                turn,
                step,
                message: assistant_to_log(&message),
                usage: None,
                interrupted: interrupted.then_some(true),
            }))
            .await?;

        if let Some(err) = failure {
            anyhow::bail!(err);
        }
        Ok(message)
    }

    async fn run_tool(
        &self,
        session: &mut Session,
        turn: u64,
        step: u64,
        call: &pai_llm::ToolCall,
        sink: &dyn TurnSink,
    ) -> anyhow::Result<()> {
        session
            .append(SessionEvent::ToolCall(LogToolCall {
                turn,
                step,
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            }))
            .await?;
        sink.tool_start(&call.id, &call.name, &call.arguments);

        // Tham số hỏng không phải lỗi của ta: một mô hình nhỏ phát ra JSON không đóng
        // ngoặc là chuyện thường. Đưa nguyên `null` xuống để đường ống trả về một câu
        // giải thích mà mô hình đọc được, thay vì làm đứt lượt ở đây.
        let arguments = serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
        let outcome = self.tools.execute(&call.id, &call.name, arguments).await;

        let preview: String = outcome.content.chars().take(200).collect();
        sink.tool_end(
            &call.id,
            &call.name,
            outcome.is_error,
            &preview,
            &outcome.meta,
        );

        session
            .append_surface(SessionEvent::ToolResult(LogToolResult {
                turn,
                step,
                call_id: call.id.clone(),
                message: LogMessage {
                    role: Role::Tool,
                    content: vec![LogBlock::ToolResult {
                        call_id: call.id.clone(),
                        content: outcome.content.clone(),
                        is_error: outcome.is_error,
                    }],
                    source: None,
                },
                error: None,
                meta: (!outcome.meta.is_empty())
                    .then(|| serde_json::Value::Object(outcome.meta.clone())),
            }))
            .await?;
        Ok(())
    }
}
