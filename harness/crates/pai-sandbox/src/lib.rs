//! Giam tiến trình: một seam, ba chế độ, một bản cài đặt cho mỗi hệ điều hành.
//!
//! Ba câu dưới đây là toàn bộ nội dung của crate này, và cả ba đều là câu phủ định.
//!
//! **Sandbox ở đây chỉ quản *hiệu ứng lên tệp*.** Không chế độ nào chặn mạng, trên bất
//! kỳ hệ điều hành nào. macOS thì `(deny network*)` phá `cargo` và `npm` tới mức không
//! ai bật; Linux cần seccomp riêng; Windows cần một tài khoản riêng làm principal cho
//! luật tường lửa. Một lệnh bị giam vẫn tải được mọi thứ về và vẫn gửi được mọi thứ đi.
//! Đây là giới hạn đã biết, không phải việc còn dở: viết nó ra đây để không ai đọc chữ
//! "sandbox" rồi suy ra một bảo đảm không tồn tại.
//!
//! **Hai nền tảng không giam giống hệt nhau.** macOS mở đúng một cống `/dev/null`; Linux
//! phải mở cả thư mục `/dev` cho quyền cấp tệp, vì Landlock không quản device node theo
//! từng tệp. Cả hai đều không tạo hay xoá được mục trong `/dev`. Ghi ra đây vì một bảng
//! nói "cả hai đều `Full`" mà không nói chỗ khác nhau là một bảng nói dối bằng cách im
//! lặng.
//!
//! **Sandbox không quản việc *đọc*.** Cả ba chế độ đều cho đọc toàn máy. Một coding
//! agent phải đọc được repo, toolchain, cache phụ thuộc và cấu hình git; đục đủ lỗ để
//! nó chạy được thì ranh giới đọc chẳng còn nghĩa gì. Bí mật nằm trong `~/.ssh` vẫn đọc
//! được — thứ chặn nó là danh sách đường dẫn được bảo vệ của `pai-fs`, không phải chỗ này.
//!
//! **Sandbox không tự nhận là đang giam.** [`Enforcement`] là *sự thật báo cáo*, không
//! phải lời hứa. Một sandbox nói dối nguy hiểm hơn hẳn không có sandbox: người dùng bấm
//! "cho phép" vì tin rằng có vòng vây, nên một vòng vây không tồn tại còn tệ hơn một
//! vòng vây không được nhắc tới. Vì vậy mọi provider ở đây trả về `None` kèm lý do thay
//! vì trả về `Full` cho chắc.
//!
//! # Bản đồ hệ điều hành
//!
//! | | Chặn ghi ngoài workspace | Cách làm |
//! |---|---|---|
//! | macOS | có, `Full` | `sandbox-exec` với hồ sơ SBPL sinh động ([`seatbelt`]) |
//! | Linux | có, `Full`/`Partial` theo ABI kernel | Landlock qua một binary trung gian ([`landlock`]) |
//! | Windows | chưa | [`Enforcement::None`] kèm lý do ([`unconfined`]) |
//!
//! Trên máy không thuộc ba nhóm trên, provider cũng là một bản `None` có lý do — chứ
//! không phải không có provider nào, vì "không ai trả lời" và "trả lời là không giam
//! được" là hai câu khác nhau đối với hộp thoại duyệt.

pub mod plugin;
pub mod policy;
pub mod seam;

#[cfg(target_os = "macos")]
pub mod seatbelt;

#[cfg(target_os = "linux")]
pub mod landlock;

pub mod unconfined;

pub use plugin::SandboxPlugin;
pub use policy::{Mode, Policy, writable_roots};
pub use seam::{Enforcement, Sandbox, SandboxError, SandboxProvider};
pub use unconfined::Unconfined;
