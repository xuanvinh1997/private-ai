//! Seam của crate này.
//!
//! Hai: cái hub (những server đang chạy) và cái kho (những server người dùng đã khai).
//! Phía server không có seam vì nó không phải một khả năng ai đó *dùng* — nó là một cái
//! cổng, và một cái cổng thì được mở hoặc không, chứ không được thay bằng một bản cài đặt
//! khác.

use pai_core::ServiceKey;

use crate::hub::McpHub;
use crate::store::McpStore;

/// Mọi server bên thứ ba. Không có provider = không có tool ngoài nào, và mọi thứ khác
/// vẫn chạy — đó chính là ý nghĩa của best-effort, viết ra trong hệ kiểu.
pub enum Mcp {}
impl ServiceKey for Mcp {
    type Api = McpHub;
    const NAME: &'static str = "mcp";
}

/// Danh sách server người dùng tự quản.
///
/// Tách khỏi [`Mcp`] vì hai thứ trả lời hai câu khác nhau: hub nói *cái gì đang chạy*, kho
/// nói *cái gì người dùng muốn chạy*. Một server tắt chỉ tồn tại ở vế thứ hai, và màn hình
/// quản lý MCP phải vẽ được nó — nên nó phải lấy được cả hai chỗ, chứ không phải suy vế
/// này ra từ vế kia.
pub enum McpConfig {}
impl ServiceKey for McpConfig {
    type Api = McpStore;
    const NAME: &'static str = "mcp.store";
}
