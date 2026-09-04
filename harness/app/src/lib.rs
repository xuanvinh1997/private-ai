//! The Tauri shell around the harness, deliberately thin: UI commands, window-only state (pending approvals,
//! cancellable turns), and the translation from loop events to UI events. All real behaviour lives in `pai-*`
//! so the core runs outside Tauri, in tests and headless.

mod approval;
pub mod coalesce;
mod commands;
pub mod harness;
mod llm;
pub mod protocol;
mod rag_config;
pub mod scope;

use std::collections::HashMap;
use std::sync::Arc;

use pai_agent::{Driver, Prompt, TurnSink};
use pai_session::{NewSession, SessionEvent, SessionScope};
use pai_tools::{ToolPipeline, Tools};
use parking_lot::Mutex;
use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use crate::approval::Approvals;
use crate::coalesce::Coalescer;
use crate::harness::{Config, Harness};
use crate::protocol::{
    AgentEvent, ApprovalDecision, HistoryNode, ModelChoice, SessionSummary, ToolScope,
};

#[derive(Default)]
pub(crate) struct AppState {
    approvals: Arc<Approvals>,
    /// Built once, on the first command: building in `setup` would turn a config error into a window that never opens.
    harness: OnceCell<Arc<Harness>>,
    /// One cancellation token per running turn, keyed by session.
    pub(crate) running: Mutex<HashMap<String, CancellationToken>>,
    /// The running clone, if any; one, not a map, because a user clones one repository at a time.
    pub(crate) cloning: Mutex<Option<CancellationToken>>,
}

impl AppState {
    pub(crate) async fn harness(&self) -> Result<Arc<Harness>, String> {
        self.harness
            .get_or_try_init(|| async {
                harness::boot(Config::from_env())
                    .await
                    .map(Arc::new)
                    .map_err(|err| err.to_string())
            })
            .await
            .cloned()
    }
}

/// Translate loop events into UI events, carrying the model's context window because `TurnSink::usage` only
/// gives the numerator, and without a denominator the UI cannot answer "how much room is left".
struct ChannelSink {
    events: Coalescer,
    context_window: Option<u64>,
}

impl TurnSink for ChannelSink {
    fn token(&self, text: &str) {
        self.events.send(AgentEvent::Token {
            text: text.to_string(),
        });
    }

    fn tool_start(&self, call_id: &str, name: &str, arguments: &str) {
        self.events.send(AgentEvent::ToolStart {
            call_id: call_id.to_string(),
            name: name.to_string(),
            // Broken arguments must still render: a card with odd arguments beats a card with nothing.
            args: serde_json::from_str(arguments).unwrap_or_else(|_| arguments.into()),
        });
    }

    fn tool_end(
        &self,
        call_id: &str,
        name: &str,
        is_error: bool,
        preview: &str,
        meta: &serde_json::Map<String, serde_json::Value>,
    ) {
        // `meta` travels from tool to UI uninterpreted, on purpose: a presentation layer in between is one more place for the two ends to drift.
        let meta = (!meta.is_empty())
            .then(|| serde_json::from_value(serde_json::Value::Object(meta.clone())).ok())
            .flatten();
        self.events.send(AgentEvent::ToolEnd {
            call_id: call_id.to_string(),
            name: name.to_string(),
            is_error,
            preview: preview.to_string(),
            meta,
        });
    }

    fn notice(&self, message: &str) {
        self.events.send(AgentEvent::Notice {
            message: message.to_string(),
        });
    }

    fn usage(&self, input_tokens: u64, output_tokens: u64) {
        self.events.send(AgentEvent::Usage {
            input_tokens,
            output_tokens,
            context_window: self.context_window,
        });
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessage {
    pub session_id: String,
    pub text: String,
    /// Tool permissions for this turn alone. No `#[serde(default)]`: a default would let an old UI run
    /// silently at a privilege level nobody chose, whereas a missing field fails loudly at the boundary.
    pub scope: ToolScope,
}

/// Run a turn, emitting events to the UI over a `Channel` rather than `emit`: a channel belongs to one turn,
/// preserves order, and disappears with it, so concurrent turns never interleave tokens.
#[tauri::command]
async fn send_message(
    input: SendMessage,
    on_event: Channel<AgentEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let harness = state.harness().await?;
    let cancel = CancellationToken::new();
    state
        .running
        .lock()
        .insert(input.session_id.clone(), cancel.clone());

    let sink = ChannelSink {
        events: Coalescer::spawn(on_event.clone()),
        context_window: u64::try_from(harness.context_window).ok(),
    };
    // The approver holds this turn's `Channel`, so prompts return to the window that raised them.
    let approver: Arc<dyn pai_tools::Approver> = Arc::new(approval::TurnApprover::new(
        state.approvals.clone(),
        on_event.clone(),
    ));
    let result = run_turn(&harness, &input, cancel, &sink, approver).await;

    state.running.lock().remove(&input.session_id);

    // Every event of a turn leaves through exactly one path, the coalescer: sending `Final` straight to the
    // `Channel` once overtook buffered tokens, so the UI closed the block and late tokens spawned a stub message.
    state.approvals.cancel_all(|event| sink.events.send(event));

    match result {
        Ok(message_id) => sink.events.send(AgentEvent::Final { message_id }),
        // Errors leave as events, not as `Err`: the UI already opened a block, and a silent rejection leaves it hanging.
        Err(message) => sink.events.send(AgentEvent::Error { message }),
    }

    // And return only after the channel has drained: the UI treats `invoke` resolving as the end of the turn.
    sink.events.finish().await;
    Ok(())
}

/// Run a turn inside the tool scope the user chose; the scope is opened and disposed in this one function,
/// which is what makes it a turn's scope rather than a setting, even when the turn fails midway.
async fn run_turn(
    harness: &Harness,
    input: &SendMessage,
    cancel: CancellationToken,
    sink: &ChannelSink,
    approver: Arc<dyn pai_tools::Approver>,
) -> Result<String, String> {
    let turn_ctx = scope::mo_pham_vi(&harness.ctx, input.scope, approver)?;
    let result = drive_turn(harness, &turn_ctx, input, cancel, sink).await;
    // Disposal is async, so it cannot be a `Drop`; this is where the restriction is lifted.
    turn_ctx.effects().dispose().await;
    result
}

/// The body of [`run_turn`], split out so every exit path passes through exactly one scope disposal.
async fn drive_turn(
    harness: &Harness,
    turn_ctx: &pai_core::Context,
    input: &SendMessage,
    cancel: CancellationToken,
    sink: &ChannelSink,
) -> Result<String, String> {
    let mut session = harness
        .sessions
        .open(&input.session_id)
        .await
        .map_err(|err| err.to_string())?;

    // The turn number is derived from the log, not held in memory, so reopening an old session continues where it stopped.
    let turn = session
        .log()
        .events()
        .iter()
        .filter(|entry| matches!(entry.event, SessionEvent::TurnStart(_)))
        .count() as u64
        + 1;

    // The loop runs in the turn's context, not the root: both permission layers read the scope from this
    // `Context`. Provider and model are read from `harness.driver` at turn start, so a mid-turn switch applies next turn.
    let registry = harness
        .ctx
        .require::<Tools>()
        .map_err(|err| err.to_string())?;
    let prompt = harness
        .ctx
        .require::<Prompt>()
        .map_err(|err| err.to_string())?;
    let driver = Driver::new(
        turn_ctx.clone(),
        harness.driver.llm(),
        Arc::new(ToolPipeline::new(turn_ctx, registry)),
        prompt,
        harness.driver.model(),
    );

    driver
        .run_turn(
            &mut session,
            turn,
            vec![pai_session::Message::user(input.text.clone())],
            cancel,
            sink,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(turn.to_string())
}

#[tauri::command]
fn approval_result(request_id: String, decision: ApprovalDecision, state: State<'_, AppState>) {
    state.approvals.resolve(&request_id, decision);
}

/// Cancel a session's running turn; calling it with no turn running is harmless.
#[tauri::command]
fn cancel_turn(session_id: String, state: State<'_, AppState>) {
    if let Some(token) = state.running.lock().remove(&session_id) {
        token.cancel();
    }
}

/// What the running plugin tree contains: the equivalent of dsh's `--dump-config`, because in a fully
/// configurable architecture the first question is always what is actually running.
#[tauri::command]
async fn describe_harness(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let harness = state.harness().await?;
    let mut lines: Vec<String> = harness.plugins.dump().lines().map(str::to_string).collect();
    lines.push(String::new());
    lines.push("# service đang cắm".into());
    lines.extend(
        harness
            .ctx
            .mounted()
            .into_iter()
            .map(|(name, realm)| format!("{name} @ {realm:?}")),
    );
    Ok(lines)
}

/// A session's stored transcript, projected from the log rather than `derive_messages()`, which drops exactly
/// what a human reader needs: tool cards and timestamps. Two projections of one log, for two audiences.
#[tauri::command]
async fn load_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryNode>, String> {
    let harness = state.harness().await?;
    load_session_for_test(&harness, &session_id).await
}

/// The body of [`load_session`], split out because a `#[tauri::command]` cannot be called from a test.
pub async fn load_session_for_test(
    harness: &Harness,
    session_id: &str,
) -> Result<Vec<HistoryNode>, String> {
    let session = harness
        .sessions
        .open(session_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut nodes = Vec::new();
    for entry in session.log().events() {
        let at = entry.time;
        match &entry.event {
            SessionEvent::UserMessage(message) => {
                let text = text_of(&message.content);
                // An empty message stays in the log but has nothing to draw.
                if !text.is_empty() {
                    nodes.push(HistoryNode::User {
                        id: format!("s{}", entry.seq),
                        text,
                        created_at: at,
                    });
                }
            }
            SessionEvent::AssistantMessage(assistant) => {
                let text = text_of(&assistant.message.content);
                if !text.is_empty() {
                    nodes.push(HistoryNode::Assistant {
                        id: format!("s{}", entry.seq),
                        text,
                        created_at: at,
                    });
                }
            }
            SessionEvent::ToolCall(call) => nodes.push(HistoryNode::Tool {
                id: format!("s{}", entry.seq),
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                args: serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null),
                is_error: false,
                preview: String::new(),
                meta: None,
                created_at: at,
            }),
            SessionEvent::ToolResult(result) => {
                // Attach the result to the call built above, searching backwards since a call/result pair is adjacent within a step.
                let content = text_of(&result.message.content);
                let is_error = result.error.is_some();
                let meta = result
                    .meta
                    .clone()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .map(Box::new);
                if let Some(HistoryNode::Tool { call_id, preview, meta: slot, is_error: flag, .. }) =
                    nodes.iter_mut().rev().find(|node| {
                        matches!(node, HistoryNode::Tool { call_id, .. } if call_id == &result.call_id)
                    })
                {
                    let _ = call_id;
                    *preview = content.chars().take(200).collect();
                    *slot = meta;
                    *flag = is_error;
                }
            }
            _ => {}
        }
    }
    Ok(nodes)
}

/// The human-readable text inside a logged message.
fn text_of(blocks: &[pai_session::ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            pai_session::ContentBlock::Text { text } => Some(text.as_str()),
            pai_session::ContentBlock::ToolResult { content, .. } => Some(content.as_str()),
            pai_session::ContentBlock::ToolCall { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tauri::command]
async fn rename_session(
    session_id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let harness = state.harness().await?;
    harness
        .sessions
        .rename(&session_id, title.trim())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_session(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    // Cancel the running turn before deleting: deleting a session mid-write leaves a turn writing into nothing.
    if let Some(token) = state.running.lock().remove(&session_id) {
        token.cancel();
    }
    harness
        .sessions
        .delete(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// Models the server offers; an empty list means the server did not answer.
#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelChoice>, String> {
    let harness = state.harness().await?;
    Ok(harness.models().await)
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    let harness = state.harness().await?;
    // Scoped to the open project, because a session belongs to the directory it was opened in. Listing
    // every session would put the previous project's conversations in this project's sidebar - and since
    // the screen selects the newest one after a switch, the user would land back in the session they just
    // left, now labelled as the new project's.
    let workspace = harness.workspace().map(|dir| dir.display().to_string());
    let scope = match workspace.as_deref() {
        Some(path) => SessionScope::Directory(path),
        // No project open is plain conversation, and those sessions record no directory.
        None => SessionScope::Unbound,
    };
    let headers = harness
        .sessions
        .list(scope, Some(100))
        .await
        .map_err(|e| e.to_string())?;
    Ok(headers
        .into_iter()
        .map(SessionSummary::from_header)
        .collect())
}

#[tauri::command]
async fn create_session(
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<SessionSummary, String> {
    let harness = state.harness().await?;
    // A session need not belong to a project: plain conversation is valid and is what the app opens first,
    // so filling `cwd` with an arbitrary directory would record an untruth.
    let opened = NewSession {
        cwd: harness.workspace().map(|dir| dir.display().to_string()),
        ..NewSession::default()
    };
    let mut session = harness
        .sessions
        .create(opened)
        .await
        .map_err(|err| err.to_string())?;
    // An empty title stays empty: `SessionSummary` fills the display, while the log records that nobody named this session.
    if let Some(title) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        session
            .set_title(title)
            .await
            .map_err(|err| err.to_string())?;
    }
    Ok(SessionSummary::from_header(session.header().clone()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("PAI_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            send_message,
            approval_result,
            cancel_turn,
            list_sessions,
            create_session,
            describe_harness,
            load_session,
            rename_session,
            delete_session,
            list_models,
            commands::projects::list_projects,
            commands::projects::open_project,
            commands::projects::remove_project,
            commands::projects::delete_project,
            commands::projects::create_project,
            commands::projects::clone_project,
            commands::projects::cancel_clone,
            commands::projects::close_project,
            commands::projects::list_dir,
            commands::projects::import_project_files,
            commands::projects::set_project_kind,
            commands::providers::list_providers,
            commands::providers::provider_presets,
            commands::providers::save_provider,
            commands::providers::remove_provider,
            commands::providers::set_active_provider,
            commands::providers::set_provider_model,
            commands::providers::probe_provider,
            commands::providers::provider_models,
            commands::providers::embedding_setting,
            commands::providers::set_embedding,
            commands::providers::probe_embedding,
            commands::mcp::list_mcp_servers,
            commands::mcp::mcp_catalog,
            commands::mcp::save_mcp_server,
            commands::mcp::remove_mcp_server,
            commands::mcp::set_mcp_enabled,
            commands::mcp::reload_mcp_servers,
            commands::complete::complete_paths,
            commands::attach::resolve_attachments,
            commands::docs::list_documents,
            commands::docs::sync_library,
            commands::docs::reprocess_library,
            commands::docs::library_stats,
            commands::docs::add_documents,
            commands::docs::remove_document,
            commands::docs::search_documents,
            commands::rerank::rerank_setting,
            commands::rerank::set_rerank,
            commands::suggest::prompt_seeds,
            commands::system::sandbox_status,
            commands::system::list_hooks,
            commands::system::hook_config_path
        ])
        .build(tauri::generate_context!())
        .expect("không khởi động được cửa sổ ứng dụng")
        .run(|app, event| {
            // Process exit cleans up most things, but not those needing a goodbye: an LSP `shutdown`, a polite
            // MCP close, a background job's process group. Blocking here is fine -- the window is already closed.
            if let tauri::RunEvent::Exit = event
                && let Some(harness) = app.state::<AppState>().harness.get()
            {
                let harness = harness.clone();
                tauri::async_runtime::block_on(async move { harness.shutdown().await });
            }
        });
}
