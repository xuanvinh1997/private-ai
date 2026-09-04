//! The turn/step loop, hand-written as a `loop` and short enough to read in one screen.
//! Everything complex hooks in from outside through [`crate::events`]. History is derived
//! from the journal, and the last round is offered no tools so no call goes unanswered.

use std::sync::Arc;

use arc_swap::ArcSwap;
use futures::FutureExt;
use futures::StreamExt;
use pai_core::Context;
use pai_llm::{ChatRequest, LlmAdapter, Message as LlmMessage, StreamChunk};
use pai_session::{
    AssistantChunk, AssistantMessage, ContentBlock as LogBlock, Message as LogMessage, Role,
    Session, SessionEvent, StepEnd, StepStart, ToolCall as LogToolCall,
    ToolResult as LogToolResult, TurnEnd, TurnEndReason, TurnStart, Usage,
};
use pai_tools::ToolPipeline;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::bridge::{assistant_to_log, to_llm_history};
use crate::events::{AgentRequest, PreStep, PreStepRequest, StepDecision, TurnStop, TurnStopping};
use crate::prompt::SystemPrompt;

/// Where the loop narrates in real time; separate from the journal, which answers what the model saw, not the user.
pub trait TurnSink: Send + Sync {
    fn token(&self, _text: &str) {}
    fn tool_start(&self, _call_id: &str, _name: &str, _arguments: &str) {}
    /// `meta` is UI-only and travels beside `content`, so a diff shown to the user costs the model no tokens.
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

    /// The finished step's tokens when the server reports them; a no-op by default, since this is telemetry.
    fn usage(&self, _input_tokens: u64, _output_tokens: u64) {}
}

/// A sink that does nothing; for headless runs and tests.
pub struct Silent;
impl TurnSink for Silent {}

/// The loop plus whatever it is talking to; `llm` and `model` sit behind [`ArcSwapAny`] so a provider swap replaces one pointer.
pub struct Driver {
    ctx: Context,
    /// An `Arc` inside an `Arc`, because `arc-swap` needs a sized `T`; the cost is one extra hop per turn.
    llm: ArcSwap<Arc<dyn LlmAdapter>>,
    tools: Arc<ToolPipeline>,
    prompt: Arc<SystemPrompt>,
    model: ArcSwap<String>,
    /// The step ceiling within one turn.
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
            llm: ArcSwap::from_pointee(llm),
            tools,
            prompt,
            model: ArcSwap::from_pointee(model.into()),
            max_steps: 12,
        }
    }

    /// Change provider; takes effect from the next turn, not the next step — see [`Driver::drive`].
    pub fn set_llm(&self, llm: Arc<dyn LlmAdapter>) {
        self.llm.store(Arc::new(llm));
    }

    /// Change model, with the same timing rule as [`Driver::set_llm`].
    pub fn set_model(&self, model: impl Into<String>) {
        self.model.store(Arc::new(model.into()));
    }

    /// The currently pinned provider; for status screens and tests.
    pub fn llm(&self) -> Arc<dyn LlmAdapter> {
        Arc::clone(&self.llm.load())
    }

    /// The currently pinned model.
    pub fn model(&self) -> String {
        self.model.load().as_str().to_string()
    }

    pub fn with_max_steps(mut self, steps: u64) -> Driver {
        self.max_steps = steps.max(1);
        self
    }

    /// Run a turn until nothing is outstanding.
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

        // Pin provider and model once per turn: re-reading them per step would split one conversation across two servers.
        let llm: Arc<dyn LlmAdapter> = Arc::clone(&self.llm.load());
        let model = self.model.load_full();

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
                    // Mask before the new message lands: `replace` indices refer to the projection the listener saw.
                    if let Some(replace) = replace {
                        // Say that the start of the conversation was summarised; compacting in silence just looks like forgetting.
                        let bo = replace.end.saturating_sub(replace.start);
                        sink.notice(&format!(
                            "Ngữ cảnh đã đầy: {bo} tin nhắn đầu phiên được rút gọn thành một                              bản tóm tắt. Chi tiết trong phần đó không còn nguyên văn — hỏi                              lại nếu cần."
                        ));
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
                    // The turn still closes normally and spends no step; the record must remember the attempt.
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
                .one_step(
                    session,
                    turn,
                    step,
                    last_round,
                    &cancel,
                    sink,
                    llm.as_ref(),
                    &model,
                )
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
                    // Unreachable: the last round advertises no tool schema to call.
                    return Ok(TurnEndReason::MaxSteps);
                }
                continue;
            }

            // No tool work outstanding; ask whether anyone wants to add more.
            match self.ctx.first::<TurnStop>(&TurnStopping { turn }).await {
                Some(more) if !more.is_empty() => pending = more,
                _ => return Ok(TurnEndReason::Completed),
            }
            if step >= self.max_steps {
                return Ok(TurnEndReason::MaxSteps);
            }
        }
    }

    /// One model call, from building the request to journalling the message.
    #[allow(clippy::too_many_arguments)]
    async fn one_step(
        &self,
        session: &mut Session,
        turn: u64,
        step: u64,
        last_round: bool,
        cancel: &CancellationToken,
        sink: &dyn TurnSink,
        // The turn's pinned pair, passed down by `drive`; deliberately not re-read from `self`.
        llm: &dyn LlmAdapter,
        model: &str,
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
                    // The model sees the encoded name; the registry accepts both forms back.
                    pai_llm::ToolSchema::new(
                        schema.name.wire(),
                        schema.description,
                        schema.parameters,
                    )
                })
                .collect()
        };

        let mut request = ChatRequest::new(model)
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
        let mut stream = llm.stream(request);
        let mut interrupted = false;
        let mut failure = None;

        loop {
            tokio::select! {
                // Cancelling does not leave the function: the partial reply still has to be journalled below.
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
        // Journal the token counts the server reported, or nothing can say what a turn cost or how full the context is.
        let usage = assembler.usage().map(|counted| Usage {
            input_tokens: counted.input_tokens,
            output_tokens: counted.output_tokens,
            // Not every server separates cache reads; `None` rather than 0, which would read as "the cache did not help".
            cached_input_tokens: None,
        });
        if let Some(counted) = usage {
            sink.usage(counted.input_tokens, counted.output_tokens);
        }
        session
            .append_surface(SessionEvent::AssistantMessage(AssistantMessage {
                turn,
                step,
                message: assistant_to_log(&message),
                usage,
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

        // Malformed arguments are common from small models; pass `null` down so the pipeline explains it instead of breaking the turn.
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
