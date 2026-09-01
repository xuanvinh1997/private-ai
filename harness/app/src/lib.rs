//! Vỏ Tauri của harness.
//!
//! Tệp này cố tình mỏng. Nó giữ đúng ba thứ: các lệnh giao diện gọi, trạng thái chỉ có
//! nghĩa với cửa sổ (câu hỏi duyệt nào đang treo, lượt nào huỷ được), và phép dịch từ sự
//! kiện của vòng lặp sang sự kiện giao diện đọc. Mọi hành vi thật nằm trong `pai-*`, để
//! đổi được lõi mà không đụng vỏ — và để lõi chạy được ngoài Tauri, trong test và ở chế
//! độ không giao diện.

mod approval;
pub mod coalesce;
mod commands;
pub mod harness;
mod llm;
pub mod protocol;

use std::collections::HashMap;
use std::sync::Arc;

use pai_agent::TurnSink;
use pai_session::{NewSession, SessionEvent};
use parking_lot::Mutex;
use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use crate::approval::Approvals;
use crate::coalesce::Coalescer;
use crate::harness::{Config, Harness};
use crate::protocol::{AgentEvent, ApprovalDecision, HistoryNode, ModelChoice, SessionSummary};

#[derive(Default)]
pub(crate) struct AppState {
    approvals: Arc<Approvals>,
    /// Dựng một lần, lúc lệnh đầu tiên tới. Dựng trong `setup` thì một lỗi cấu hình sẽ
    /// hiện ra dưới dạng cửa sổ không mở được, không kèm lý do nào người dùng đọc được.
    harness: OnceCell<Arc<Harness>>,
    /// Một token huỷ cho mỗi lượt đang chạy, tra theo phiên.
    pub(crate) running: Mutex<HashMap<String, CancellationToken>>,
    /// Bản clone đang chạy, nếu có.
    ///
    /// Một cái, không phải một bảng: người dùng clone một repo tại một thời điểm, và một
    /// bảng ở đây sẽ đòi giao diện phải sinh và giữ một khoá cho một thứ chỉ có một.
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

/// Dịch sự kiện của vòng lặp sang sự kiện giao diện.
struct ChannelSink(Coalescer);

impl TurnSink for ChannelSink {
    fn token(&self, text: &str) {
        self.0.send(AgentEvent::Token {
            text: text.to_string(),
        });
    }

    fn tool_start(&self, call_id: &str, name: &str, arguments: &str) {
        self.0.send(AgentEvent::ToolStart {
            call_id: call_id.to_string(),
            name: name.to_string(),
            // Tham số hỏng vẫn phải hiện được: giao diện vẽ một thẻ có tham số lạ tốt hơn
            // một thẻ không có gì.
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
        // `meta` đi từ tool tới giao diện mà không ai ở giữa diễn giải nó. Cố ý: dsh có
        // `presentCall`/`presentResult` ở phía host và bản web của nó **không dùng** —
        // một tầng trình bày ở giữa là một tầng nữa để hai đầu lệch pha.
        let meta = (!meta.is_empty())
            .then(|| serde_json::from_value(serde_json::Value::Object(meta.clone())).ok())
            .flatten();
        self.0.send(AgentEvent::ToolEnd {
            call_id: call_id.to_string(),
            name: name.to_string(),
            is_error,
            preview: preview.to_string(),
            meta,
        });
    }

    fn notice(&self, message: &str) {
        self.0.send(AgentEvent::Notice {
            message: message.to_string(),
        });
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessage {
    session_id: String,
    text: String,
}

/// Chạy một lượt, phát sự kiện về giao diện.
///
/// Dùng `Channel` chứ không `emit`: một channel gắn với đúng một lượt, giữ thứ tự, và tự
/// dọn khi giao diện bỏ nó — nên hai lượt song song không trộn token vào nhau, và không
/// listener nào sống sót qua một lượt đã kết thúc.
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

    let sink = ChannelSink(Coalescer::spawn(on_event.clone()));
    let result = run_turn(&harness, &input, cancel, &sink).await;

    state.running.lock().remove(&input.session_id);

    // Mọi sự kiện của lượt đi qua **đúng một** đường ra — bộ gộp. Trước đây `Final`,
    // `Error` và `ApprovalCancel` gửi thẳng vào `Channel` trong khi token đi qua bộ đệm
    // 16 ms, nên chúng vượt lên trước những token cuối cùng: giao diện thấy `final`, đóng
    // khối trả lời, rồi token muộn tới và đẻ ra một tin nhắn cụt mang con trỏ nhấp nháy
    // không bao giờ tắt. Bộ gộp xả hết token trước khi cho bất kỳ sự kiện nào khác đi qua,
    // nên chỉ cần đi chung đường là thứ tự đúng.
    state.approvals.cancel_all(|event| sink.0.send(event));

    match result {
        Ok(message_id) => sink.0.send(AgentEvent::Final { message_id }),
        // Lỗi đi ra bằng đường sự kiện chứ không bằng `Err`: giao diện đã dựng một khối
        // cho lượt này rồi, và một lời từ chối im lặng để nó treo ở đó mãi.
        Err(message) => sink.0.send(AgentEvent::Error { message }),
    }

    // Và trả về **sau khi** kênh đã nhận hết: `invoke` resolve là tín hiệu giao diện dùng
    // để kết thúc lượt, nên nó không được sớm hơn sự kiện cuối cùng.
    sink.0.finish().await;
    Ok(())
}

async fn run_turn(
    harness: &Harness,
    input: &SendMessage,
    cancel: CancellationToken,
    sink: &ChannelSink,
) -> Result<String, String> {
    let mut session = harness
        .sessions
        .open(&input.session_id)
        .await
        .map_err(|err| err.to_string())?;

    // Số lượt suy ra từ sổ, không giữ trong bộ nhớ: mở lại một phiên cũ phải tiếp đúng
    // chỗ nó dừng, kể cả sau khi ứng dụng đã đóng.
    let turn = session
        .log()
        .events()
        .iter()
        .filter(|entry| matches!(entry.event, SessionEvent::TurnStart(_)))
        .count() as u64
        + 1;

    harness
        .driver
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

/// Huỷ lượt đang chạy của một phiên. Gọi khi không có lượt nào thì không sao.
#[tauri::command]
fn cancel_turn(session_id: String, state: State<'_, AppState>) {
    if let Some(token) = state.running.lock().remove(&session_id) {
        token.cancel();
    }
}

/// Cây plugin đang chạy gồm những gì.
///
/// Tương đương `--dump-config` của dsh, và có mặt vì cùng một lý do: trong một kiến trúc
/// mà mọi thứ đều thay được từ cấu hình, câu hỏi đầu tiên khi có gì đó sai luôn là "bản
/// đang chạy thật sự gồm những gì".
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

/// Bản ghi đã lưu của một phiên, dựng lại từ sổ.
///
/// Chiếu **sổ**, không chiếu `derive_messages()`: phép chiếu kia bỏ đi mọi thứ mô hình
/// không thấy — thẻ tool, giờ giấc — mà đó chính là những thứ người đọc cần. Hai phép
/// chiếu từ cùng một sổ, cho hai người đọc khác nhau.
#[tauri::command]
async fn load_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryNode>, String> {
    let harness = state.harness().await?;
    load_session_for_test(&harness, &session_id).await
}

/// Thân của [`load_session`], tách ra vì một `#[tauri::command]` không gọi được từ test.
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
                // Message rỗng vẫn nằm trong sổ nhưng không có gì để vẽ.
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
                // Kết quả gắn vào đúng lời gọi đã dựng ở trên. Tìm ngược vì một cặp
                // gọi/kết quả luôn kề nhau trong một bước.
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

/// Văn bản người đọc thấy được trong một message của sổ.
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
    // Huỷ lượt đang chạy trước khi xoá: xoá một phiên trong lúc nó đang ghi để lại một
    // lượt viết vào chỗ không còn ai nhận.
    if let Some(token) = state.running.lock().remove(&session_id) {
        token.cancel();
    }
    harness
        .sessions
        .delete(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// Mô hình máy chủ đang có. Danh sách rỗng nghĩa là máy chủ không trả lời được.
#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelChoice>, String> {
    let harness = state.harness().await?;
    Ok(harness.models().await)
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    let harness = state.harness().await?;
    let headers = harness
        .sessions
        .list(Some(100))
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
    // Phiên **không** bắt buộc thuộc một dự án. Một phiên trò chuyện thuần tuý là một
    // phiên hợp lệ, và đó là thứ ứng dụng mở lên lần đầu; điền đại một thư mục vào `cwd`
    // chỉ để trường ấy có giá trị là ghi vào sổ một điều không đúng.
    let opened = NewSession {
        cwd: harness.workspace().map(|dir| dir.display().to_string()),
        ..NewSession::default()
    };
    let mut session = harness
        .sessions
        .create(opened)
        .await
        .map_err(|err| err.to_string())?;
    // Tiêu đề rỗng thì để trống hẳn: `SessionSummary` tự điền chỗ hiển thị, còn sổ giữ
    // đúng sự thật là chưa ai đặt tên cho phiên này.
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
            commands::projects::create_project,
            commands::projects::clone_project,
            commands::projects::cancel_clone,
            commands::projects::close_project,
            commands::providers::list_providers,
            commands::providers::provider_presets,
            commands::providers::save_provider,
            commands::providers::remove_provider,
            commands::providers::set_active_provider,
            commands::providers::set_provider_model,
            commands::providers::probe_provider,
            commands::providers::embedding_setting,
            commands::providers::set_embedding,
            commands::providers::probe_embedding,
            commands::mcp::list_mcp_servers,
            commands::mcp::mcp_catalog,
            commands::mcp::save_mcp_server,
            commands::mcp::remove_mcp_server,
            commands::mcp::set_mcp_enabled,
            commands::mcp::reload_mcp_servers,
            commands::docs::list_documents,
            commands::docs::library_stats,
            commands::docs::add_documents,
            commands::docs::remove_document,
            commands::docs::search_documents
        ])
        .build(tauri::generate_context!())
        .expect("không khởi động được cửa sổ ứng dụng")
        .run(|app, event| {
            // Thoát tiến trình dọn được phần lớn mọi thứ, nhưng không dọn được thứ cần
            // nói lời tạm biệt: một `shutdown` gửi cho language server, một phiên MCP
            // đóng tử tế, một job nền bị giết cả nhóm. Chặn ở đây là chấp nhận được —
            // cửa sổ đã đóng rồi, không ai đang chờ.
            if let tauri::RunEvent::Exit = event
                && let Some(harness) = app.state::<AppState>().harness.get()
            {
                let harness = harness.clone();
                tauri::async_runtime::block_on(async move { harness.shutdown().await });
            }
        });
}
