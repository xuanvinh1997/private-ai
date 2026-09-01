//! Dự án: một thư mục, và mọi thứ gắn với nó.
//!
//! Trước khi có tệp này, ứng dụng có đúng **một** thư mục làm việc, chốt lúc khởi động từ
//! biến môi trường. Bảy plugin bắt lấy giá trị đó lúc dựng, và không có đường nào đổi.
//! Một coding agent như thế chỉ dùng được cho một repo mỗi lần chạy.
//!
//! Dự án là câu trả lời, và điểm đáng nói không nằm ở crate này mà ở chỗ **đổi dự án được
//! hiện thực thế nào**: nhánh plugin thuộc dự án bị tháo rồi cắm lại với đường dẫn mới.
//! Không có đường "cấu hình lại mọi thứ" nào song song — nếu phải viết một đường như thế
//! thì kiến trúc plugin đã sai từ đầu. Xem `Harness::open_project` bên `pai-app`.
//!
//! **Danh tính của một dự án là đường dẫn đã chuẩn hoá**, không phải cái tên. Hai lối vào
//! cùng một thư mục — qua symlink, qua `..` — phải là một dự án, nếu không người dùng sẽ
//! có hai hàng trong danh sách trỏ cùng một chỗ, mỗi hàng nhớ một nửa lịch sử.

mod store;
mod tree;

pub use store::{Project, ProjectError, ProjectStore, SqliteProjectStore};
pub use tree::{FileView, TreeEntry, list_tree, read_file};
