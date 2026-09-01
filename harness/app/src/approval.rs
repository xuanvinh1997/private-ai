//! Hỏi người dùng có cho chạy một tool hay không.
//!
//! Luật duy nhất đáng nhớ: **không trả lời được là từ chối.** Webview chết, người dùng
//! đóng cửa sổ, kênh đứt, hết giờ — tất cả đều ra cùng một kết quả. Hạn giờ trong hộp
//! thoại chỉ là lớp thứ hai; nếu lõi tin vào nó thì một webview chết sẽ treo cả lượt.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

use crate::protocol::{AgentEvent, ApprovalDecision};

/// Bao lâu thì coi như người dùng đã bỏ đi. Một hộp thoại đứng mãi chặn cả lượt.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Approvals {
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>,
}

// `ask` và `cancel_all` là nửa dành cho lõi: vòng lặp agent gọi chúng khi một tool xin
// duyệt. Chúng chưa có nơi gọi cho tới khi `pai-agent` được nối vào, nhưng viết sẵn ở
// đây vì hợp đồng fail-closed phải nằm cùng chỗ với phần giao diện trả lời.
#[allow(dead_code)]
impl Approvals {
    /// Hỏi, rồi chờ. Trả về quyết định — không bao giờ trả lỗi, vì mọi đường hỏng đều
    /// quy về `Rejected`.
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
            // Kênh đã đứt: không ai nghe câu hỏi, nên không ai trả lời được.
            self.pending.lock().remove(&request_id);
            return ApprovalDecision::Rejected;
        }

        let decision = match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(decision)) => decision,
            // Hết giờ, hoặc đầu gửi bị thả vì lượt đã huỷ.
            _ => ApprovalDecision::Rejected,
        };
        self.pending.lock().remove(&request_id);
        decision
    }

    /// Giao diện trả lời. Câu trả lời cho một yêu cầu đã hết hạn bị bỏ qua trong im
    /// lặng — nó không còn chỗ nào để đi.
    pub fn resolve(&self, request_id: &str, decision: ApprovalDecision) {
        if let Some(tx) = self.pending.lock().remove(request_id) {
            let _ = tx.send(decision);
        }
    }

    /// Huỷ mọi câu hỏi đang treo. Thả đầu gửi làm bên chờ tỉnh dậy ngay với `Rejected`.
    /// Rút lại mọi câu hỏi đang treo.
    ///
    /// Nhận một hàm gửi chứ không nhận `Channel`: sự kiện của một lượt phải đi qua **đúng
    /// một** đường ra, và đường đó là bộ gộp. Gửi thẳng vào `Channel` ở đây là chen ngang
    /// trước những token còn trong bộ đệm, và thứ tự sai thì giao diện đóng nhầm khối.
    pub fn cancel_all(&self, send: impl Fn(AgentEvent)) {
        for (request_id, tx) in self.pending.lock().drain() {
            drop(tx);
            send(AgentEvent::ApprovalCancel { request_id });
        }
    }
}
