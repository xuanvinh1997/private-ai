//! Seam của crate này.
//!
//! Ba thứ mà đường ống cần nhưng không được phép tự cài: ai hỏi người dùng cho phép, ai
//! hỏi người dùng một giá trị, và cất phần output tràn ra ở đâu. Cả ba đều tra ở
//! `Context` **tại thời điểm gọi**, không cache vào struct — gỡ tải cái hộp thoại phê
//! duyệt phải làm mọi lần hỏi sau đó thành từ chối, chứ không phải làm chúng đi qua một
//! bản sao còn sót lại.

use pai_core::ServiceKey;

use crate::pipeline::Approver;
use crate::registry::ToolRegistry;
use crate::spill::SpillStore;
use crate::tool::Elicitor;

/// Sổ đăng ký tool.
pub enum Tools {}
impl ServiceKey for Tools {
    type Api = ToolRegistry;
    const NAME: &'static str = "tools";
}

/// Hỏi cho phép. Không có provider = từ chối mọi thứ cần hỏi.
pub enum Approval {}
impl ServiceKey for Approval {
    type Api = dyn Approver;
    const NAME: &'static str = "tools/approval";
}

/// Hỏi một giá trị. Không có provider = không hỏi được.
pub enum Elicitation {}
impl ServiceKey for Elicitation {
    type Api = dyn Elicitor;
    const NAME: &'static str = "tools/elicitation";
}

/// Kho tràn. Không có provider = không cắt gì cả, output dài đi nguyên vẹn.
pub enum Spill {}
impl ServiceKey for Spill {
    type Api = dyn SpillStore;
    const NAME: &'static str = "tools/spill";
}
