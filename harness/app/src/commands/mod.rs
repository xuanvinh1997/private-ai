//! Lệnh mà giao diện gọi, chia theo màn hình.
//!
//! Tách khỏi `lib.rs` khi số lệnh vượt qua chỗ đọc được trong một lần cuộn. Ranh giới là
//! **màn hình**, không phải crate lõi: một màn hình hỏng thì người sửa mở đúng một tệp,
//! còn chia theo crate thì màn hình dự án nằm rải ở ba chỗ vì nó gọi ba crate.
//!
//! Mọi lệnh ở đây theo cùng một quy ước lỗi: trả `Result<_, String>`, và chuỗi lỗi là
//! **câu người dùng đọc được**, không phải `Debug` của một kiểu lỗi. Giao diện hiện thẳng
//! nó ra, nên một `NotADirectory("/x")` lọt qua đây là một dòng tiếng Anh trong một ứng
//! dụng tiếng Việt.

pub mod docs;
pub mod graph;
pub mod mcp;
pub mod projects;
pub mod providers;
