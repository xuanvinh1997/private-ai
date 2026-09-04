//! Asking the user whether a tool may run. The one rule: no answer means denial -- a dead webview, a closed
//! window, a broken channel and a timeout all end the same way. The dialog's own timer is only a second layer.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

use crate::protocol::{AgentEvent, ApprovalDecision};

/// How long before we assume the user walked away; a dialog left open blocks the whole turn.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Approvals {
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>,
}

impl Approvals {
    /// Ask, then wait. Always returns a decision, never an error: every failure path collapses to `Rejected`.
    pub async fn ask(
        self: &Arc<Self>,
        channel: &Channel<AgentEvent>,
        call_id: &str,
        name: &str,
        args: serde_json::Value,
        reason: Option<String>,
    ) -> ApprovalDecision {
        let request_id = uuid::Uuid::now_v7().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id.clone(), tx);

        let sent = channel.send(AgentEvent::ApprovalRequest {
            request_id: request_id.clone(),
            call_id: call_id.to_string(),
            name: name.to_string(),
            args,
            reason,
            timeout_ms: Some(DEFAULT_TIMEOUT.as_millis() as u64),
        });
        if sent.is_err() {
            // The channel is gone: nobody hears the question, so nobody can answer it.
            self.pending.lock().remove(&request_id);
            return ApprovalDecision::Rejected;
        }

        let decision = match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(decision)) => decision,
            // Timed out, or the sender was dropped because the turn was cancelled.
            _ => ApprovalDecision::Rejected,
        };
        self.pending.lock().remove(&request_id);
        decision
    }

    /// The UI answers; a reply to an expired request is dropped silently, since it has nowhere to go.
    pub fn resolve(&self, request_id: &str, decision: ApprovalDecision) {
        if let Some(tx) = self.pending.lock().remove(request_id) {
            let _ = tx.send(decision);
        }
    }

    /// Withdraw every pending question; takes a send function rather than a `Channel`, because a turn's events
    /// must all leave through the coalescer or they overtake buffered tokens and the UI closes the wrong block.
    pub fn cancel_all(&self, send: impl Fn(AgentEvent)) {
        for (request_id, tx) in self.pending.lock().drain() {
            drop(tx);
            send(AgentEvent::ApprovalCancel { request_id });
        }
    }
}

/// Bridge between the core [`Approval`] seam and the window's dialog. Built per turn, not once at startup,
/// because a prompt must leave through the `Channel` of the turn that raised it. It exists because
/// `Approvals` was once never wired into the seam, and fail-closed approval made `bash` silently unusable.
pub struct TurnApprover {
    approvals: Arc<Approvals>,
    channel: Channel<AgentEvent>,
}

impl TurnApprover {
    pub fn new(approvals: Arc<Approvals>, channel: Channel<AgentEvent>) -> TurnApprover {
        TurnApprover { approvals, channel }
    }
}

#[async_trait::async_trait]
impl pai_tools::Approver for TurnApprover {
    async fn approve(&self, request: &pai_tools::ApprovalRequest) -> bool {
        let decision = self
            .approvals
            .ask(
                &self.channel,
                &request.call_id,
                &request.name.to_string(),
                serde_json::Value::Object(request.arguments.clone()),
                Some(request.reason.clone()),
            )
            .await;
        decision == ApprovalDecision::AllowedOnce
    }
}
