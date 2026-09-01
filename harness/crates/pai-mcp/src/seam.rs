//! Seam của crate này.
//!
//! Chỉ một: cái hub. Phía server không có seam vì nó không phải một khả năng ai đó *dùng*
//! — nó là một cái cổng, và một cái cổng thì được mở hoặc không, chứ không được thay bằng
//! một bản cài đặt khác.

use pai_core::ServiceKey;

use crate::hub::McpHub;

/// Mọi server bên thứ ba. Không có provider = không có tool ngoài nào, và mọi thứ khác
/// vẫn chạy — đó chính là ý nghĩa của best-effort, viết ra trong hệ kiểu.
pub enum Mcp {}
impl ServiceKey for Mcp {
    type Api = McpHub;
    const NAME: &'static str = "mcp";
}
