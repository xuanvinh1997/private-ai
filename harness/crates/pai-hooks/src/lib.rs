//! Hook: chính sách của người vận hành, chạy quanh mỗi lời gọi tool.
//!
//! Một hook là một lệnh ngoài. Harness đưa cho nó mô tả lời gọi qua stdin dưới dạng JSON
//! và đọc quyết định trên stdout, cũng JSON. Nhờ thế chính sách viết được bằng bất cứ thứ
//! gì — một dòng `jq`, một script Python, một binary của công ty — mà không ai phải build
//! lại ứng dụng.
//!
//! Ba quyết định đáng viết ra:
//!
//! **Hook chạy *ngoài* vòng giam.** Chúng không đi qua seam `Shell`, mà spawn thẳng. Hook
//! là chính sách của người vận hành, không phải việc của mô hình; cho vòng giam của agent
//! quyết định xem chính sách có được chạy hay không là lộn ngược quan hệ. Cái giá là hook
//! chạy với đầy đủ quyền người dùng — nhưng nó vốn đã là một lệnh do chính người dùng ghi
//! vào tệp cấu hình của mình.
//!
//! **Hook hỏng thì cho qua, hook nói "không" thì chặn.** Một hook lỗi cú pháp, hết giờ,
//! hay không có tệp là **lỗi của chính sách**, không phải bằng chứng rằng lời gọi nguy
//! hiểm; chặn mọi thứ vì một script hỏng là biến một lỗi gõ nhầm thành một ứng dụng đứng
//! im. Ngược lại, một hook chạy được và nói `deny` thì được tôn trọng tuyệt đối.
//!
//! Đây là chỗ cố ý **khác** với phê duyệt: `Approver` fail-closed vì nó thay mặt người
//! dùng đang ngồi đó, còn hook fail-open vì nó thay mặt một tệp cấu hình. Hai vai khác
//! nhau nên hai mặc định khác nhau.
//!
//! **Hook không sửa được tham số.** Nó chỉ trả `allow` hoặc `deny` kèm lý do. Cho phép
//! viết lại tham số nghe tiện, nhưng nó tạo ra một lời gọi mà cả mô hình lẫn người dùng
//! đều không nhìn thấy — và bản ghi sẽ nói dối về thứ đã thật sự chạy.

pub mod plugin;
pub mod runner;

pub use plugin::{HookConfig, HooksPlugin};
pub use runner::{HookDecision, HookInput};
