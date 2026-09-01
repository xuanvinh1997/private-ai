//! Vòng lặp agent.
//!
//! Trái tim của sản phẩm, và cố tình là phần nhỏ nhất. Nó biết đúng bốn chỗ để hỏi ý
//! kiến bên ngoài — xem [`events`] — và không biết gì về phê duyệt, sandbox, hook hay
//! nén ngữ cảnh. Thêm một chính sách là cắm một plugin, không phải sửa một vòng lặp.
//!
//! Ba bất biến, cả ba đều là chỗ đã từng sai ở bản trước:
//!
//! **Lịch sử mô hình dựng từ sổ.** Không có bản sao thứ hai trong bộ nhớ để mà lệch.
//!
//! **Vòng cuối không có tool.** Nếu chỉ chặn bằng trần số vòng thì lượt kết thúc bằng một
//! lời gọi tool không ai trả lời.
//!
//! **Huỷ giữ lại phần trả lời dở.** Nhánh huỷ không thoát ra khỏi hàm; nó dừng vòng đọc
//! rồi đi tiếp xuống chỗ ghi sổ, đúng một chỗ mà mọi đường thoát đều đi qua.

pub mod bridge;
pub mod compaction;
pub mod driver;
pub mod events;
pub mod plugin;
pub mod prompt;
pub mod skills;
pub mod subagent;

pub use compaction::CompactionPlugin;
pub use driver::{Driver, Silent, TurnSink};
pub use events::{
    AgentRequest, PreStep, PreStepRequest, Replacement, StepDecision, TurnStop, TurnStopping,
};
pub use plugin::AgentPlugin;
pub use prompt::{Prompt, SystemPrompt, order};
pub use skills::{Skill, SkillRegistry, SkillsPlugin};
pub use subagent::{
    LocalSubagents, MAX_DEPTH, SubagentPlugin, SubagentProvider, SubagentReport, Subagents, Task,
};
