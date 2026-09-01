//! Sổ đăng ký tool có phạm vi, và đường ống thi hành có canh gác.
//!
//! Đây là crate nền tảng bảo mật của sản phẩm. Mọi thứ mô hình có thể *làm* đều đi qua
//! đây, nên bốn bất biến dưới đây được viết ra trước, và mọi API trong crate được sắp xếp
//! để chỗ sai khó gõ ra hơn chỗ đúng.
//!
//! **1. Lọc hai tầng.** Quyền được kiểm lúc liệt kê *và* một lần nữa lúc gọi, sau khi đã
//! giải mã tên. Danh sách quảng cáo chỉ là gợi ý: một mô hình đoán ra `documents__delete`
//! không đi qua hàm liệt kê bao giờ. Cả hai tầng gọi cùng một
//! [`registry::ToolRegistry::permits`], nên chúng không trôi ra khỏi nhau được.
//!
//! **2. Tham số mô hình không thấy là tham số nó không thể làm sai.** Ghim một tham số
//! **xoá nó khỏi schema** rồi **ghi đè** lúc gọi. Không phải điền mặc định: một mô hình
//! tự gửi kèm `workspace_id` của workspace khác phải bị ghi đè, chứ không phải được tôn
//! trọng vì nó đã điền sẵn.
//!
//! **3. Từ chối là văn bản, không phải lỗi.** [`pipeline::ToolPipeline::execute`] không
//! trả `Result`. Một `Err` lọt lên trên chỉ kết thúc lượt trong im lặng; mô hình phải đọc
//! được vì sao nó không được chạy, nếu không nó sẽ thử lại đúng cách vừa hỏng.
//!
//! **4. Canh gác đơn điệu.** [`pipeline::ToolGuard`] chỉ từ chối hoặc bỏ qua — không có
//! nhánh cho phép — nên thứ tự đăng ký không thể biến một lệnh từ chối thành cho phép.
//!
//! Và một luật nữa, về nội dung chứ không về quyền: nếu
//! [`schema::ToolMeta::returns_untrusted_content`] bật thì lời cảnh báo được **tự chèn
//! vào mô tả tool**, vì mô tả tool là thứ duy nhất mô hình đọc đúng vào lúc nó quyết định
//! làm gì với văn bản trả về.

pub mod budget;
pub mod builtin;
pub mod name;
pub mod pipeline;
pub mod plugin;
pub mod registry;
pub mod schema;
pub mod seam;
pub mod spill;
pub mod tool;

pub use budget::{BYTES_PER_TOKEN, DEFAULT_TOKEN_BUDGET, Folded, Overflow, Split, approx_tokens};
pub use name::{ToolName, WIRE_SEPARATOR};
pub use pipeline::{
    APPROVAL_TIMEOUT, ApprovalRequest, Approver, Execute, PostDecision, PostExecute, PostRequest,
    PreDecision, PreExecute, PreRequest, ResolvedCall, ToolGuard, ToolPipeline, ToolResult,
    not_available,
};
pub use plugin::ToolsPlugin;
pub use registry::{Resolution, ToolRegistry, ToolRestriction};
pub use schema::{ToolMeta, ToolSchema, UNTRUSTED_NOTICE, json_schema_for};
pub use seam::{Approval, Elicitation, Spill, Tools};
pub use spill::{MemorySpillStore, SpillRef, SpillStore};
pub use tool::{Elicitor, Invocation, Tool, ToolError, ToolOutcome};
