//! Skill: một quy trình đóng gói sẵn.
//!
//! Một thư mục có `SKILL.md`, mở đầu bằng khối frontmatter YAML, phần còn lại là hướng
//! dẫn viết bằng markdown. Ý nghĩa thật của cơ chế này nằm ở **tiết lộ dần**, ba tầng:
//!
//! 1. Prompt hệ thống luôn mang **tên + một dòng mô tả** của mọi skill đang bật.
//! 2. **Toàn văn hướng dẫn** chỉ được chèn vào khi skill đó được chọn cho lượt hiện tại.
//! 3. Các tệp khác trong cùng thư mục chỉ được **liệt kê tên**; mô hình tự mở bằng `read`
//!    khi thật sự cần.
//!
//! Một trăm skill vì thế tốn một trăm dòng tóm tắt, không phải một trăm tài liệu. Và
//! việc chọn skill nào cho một lượt làm bằng trùng lặp từ khoá, không tốn một lần gọi
//! mô hình — một lần gọi để quyết định có nên gọi thì đã hỏng ngay ở tên gọi.
//!
//! **Skill là chỉ dẫn đáng tin cậy.** Nội dung của nó do người vận hành viết, nên nó
//! được chèn vào prompt như luật của chính ta — đúng hình ảnh phản chiếu của lời cảnh
//! báo dán lên trích đoạn tài liệu. Vì thế không có đường nào từ truy hồi hay từ mô hình
//! được phép tạo, đặt tên hay sửa một skill.

mod loader;
mod registry;

pub use loader::{Skill, SkillError, load_skill};
pub use registry::{SkillRegistry, SkillsPlugin};
