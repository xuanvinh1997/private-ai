//! Bốn điểm cắm của vòng lặp.
//!
//! Vòng lặp **không được biết gì** về phê duyệt, sandbox, hook hay nén ngữ cảnh. Nó chỉ
//! biết bốn chỗ này, và mọi chính sách đến từ bên ngoài qua chúng. Đó là toàn bộ khác
//! biệt giữa một vòng lặp thay được và một vòng lặp phải sửa mỗi lần thêm tính năng.

use pai_core::{First, Waterfall};
use pai_llm::ChatRequest;
use pai_session::Message;

/// Yêu cầu che một dải node cũ bằng một bản tóm tắt.
///
/// Không phải xoá: dải cũ vẫn nằm nguyên trong sổ và vẫn phát lại được, chỉ phép chiếu
/// ngừng nhìn thấy nó. Đó là khác biệt giữa một bản ghi rút gọn được và một bản ghi mất
/// đoạn — cái sau thì không ai dựng lại được lượt đã chạy.
#[derive(Debug, Clone)]
pub struct Replacement {
    /// Vị trí node, nửa mở.
    pub start: usize,
    pub end: usize,
    pub summary: Message,
}

/// Quyết định của `agent/pre-step`.
#[derive(Debug, Clone)]
pub enum StepDecision {
    /// Vào bước, với đúng những message này. Listener được phép sửa danh sách.
    Enter {
        messages: Vec<Message>,
        /// Che bớt lịch sử trước khi dựng request. Chạy trước khi message mới vào sổ,
        /// nên vị trí ở đây tính trên phép chiếu mà listener vừa nhìn thấy.
        replace: Option<Replacement>,
    },
    /// Không vào bước. Lượt vẫn được ghi sổ và đóng lại — bản ghi phải nhớ là đã có
    /// người thử, kể cả khi không có bước nào tiêu.
    Reject { reason: String },
}

impl StepDecision {
    /// Vào bước, không che gì. Dạng thường gặp nhất.
    pub fn enter(messages: Vec<Message>) -> StepDecision {
        StepDecision::Enter {
            messages,
            replace: None,
        }
    }
}

pub struct PreStepRequest {
    pub turn: u64,
    pub step: u64,
    pub messages: Vec<Message>,
    /// Phép chiếu hiện tại của sổ. Có mặt ở đây vì chính sách nén phải **đo** được trước
    /// khi quyết định, mà đo thì cần đúng thứ mô hình sắp thấy chứ không phải một ước
    /// lượng nào khác.
    pub history: Vec<Message>,
}

/// Cái gì được vào bước. Chính sách nén ngữ cảnh cắm ở đây.
pub enum PreStep {}
impl Waterfall for PreStep {
    const NAME: &'static str = "agent/pre-step";
    type Req = PreStepRequest;
    type Out = StepDecision;
}

/// Request cuối cùng gửi cho mô hình. Chỗ để thêm ngữ cảnh, đổi mô hình, cắt lịch sử.
pub enum AgentRequest {}
impl Waterfall for AgentRequest {
    const NAME: &'static str = "agent/request";
    type Req = ChatRequest;
    type Out = ChatRequest;
}

pub struct TurnStopping {
    pub turn: u64,
}

/// Chốt chặn cuối trước khi lượt đóng. Trả `Some` để **giữ lượt mở** và chạy tiếp một
/// bước nữa — đây là cách một plugin nối thêm việc mà không cần biết vòng lặp.
pub enum TurnStop {}
impl First for TurnStop {
    const NAME: &'static str = "agent/turn-stopping";
    type Payload = TurnStopping;
    type Out = Vec<Message>;
}
