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

/// Cầu nối giữa seam [`Approval`] của lõi và hộp thoại trong cửa sổ.
///
/// Phải dựng **theo từng lượt**, không dựng một lần lúc khởi động: câu hỏi duyệt đi ra
/// bằng chính `Channel` của lượt đã sinh ra nó. Một cầu nối dùng chung sẽ phải chọn xem
/// gửi câu hỏi tới cửa sổ nào khi hai lượt chạy song song, và mọi cách chọn đều sai.
///
/// # Vì sao tệp này từng vô dụng
///
/// `Approvals` có đủ hai nửa — hỏi và trả lời — nhưng **không ai cắm nó vào seam
/// `Approval`**. Đường ống tool thì fail-closed: không có provider nghĩa là mọi lời xin
/// duyệt đều bị từ chối. Nên `bash` chưa từng chạy được một lần nào trong sản phẩm thật,
/// và triệu chứng lại giống hệt "mô hình không biết gọi tool". Đúng luật 10 của
/// `docs/CONTRACT.md`, ở dạng tệ nhất: một khả năng có mặt trong danh sách tool, có mặt
/// trong giao diện, và không tồn tại.
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
