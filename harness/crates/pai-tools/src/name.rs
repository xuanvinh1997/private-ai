//! Tên tool: hai dạng chiếu của cùng một danh tính.
//!
//! Dạng có dấu chấm (`documents.ingest_text`) là cái MCP được hỏi và là **danh tính
//! chuẩn** — mọi quyết định về quyền đều nói bằng dạng này. Dạng `__` là cái mô hình
//! thấy, vì tên hàm trên wire format của OpenAI chỉ nhận chữ, số, gạch dưới và gạch nối:
//! một dấu chấm ở đó là một request bị từ chối.
//!
//! Hai dạng phải được giữ tách bạch trong hệ kiểu, chứ không phải trong đầu người viết.
//! Chính chỗ này là nơi bản Python suýt trượt: nếu kiểm tra quyền chạy trên dạng wire mà
//! đăng ký lại nói bằng dạng chấm thì hai bên so sánh hai thứ khác nhau, và cái tưởng là
//! một bộ lọc thì thật ra không lọc gì cả.

use std::fmt;

use serde::Serialize;

/// Cái thay cho dấu chấm trên wire.
pub const WIRE_SEPARATOR: &str = "__";

/// Danh tính chuẩn của một tool — luôn ở dạng có dấu chấm.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolName(String);

impl ToolName {
    /// Dựng từ dạng chuẩn (có dấu chấm).
    pub fn new(name: impl Into<String>) -> ToolName {
        ToolName(name.into())
    }

    /// Dạng chuẩn — cái mà sổ đăng ký, restriction và log đều nói.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Dạng mô hình thấy.
    pub fn wire(&self) -> String {
        self.0.replace('.', WIRE_SEPARATOR)
    }

    /// Giải mã một tên đến từ mô hình.
    ///
    /// Đây là **phép giải mã**, không phải phép tra cứu: nó không hứa rằng tool tồn tại,
    /// cũng không hứa rằng nó được phép. Cả hai câu hỏi đó thuộc về sổ đăng ký, và phải
    /// được hỏi lại sau khi gọi hàm này — xem [`crate::registry::ToolRegistry::resolve`].
    pub fn from_wire(wire: &str) -> ToolName {
        ToolName(wire.replace(WIRE_SEPARATOR, "."))
    }

    /// Phép chiếu chỉ khả nghịch khi dạng chuẩn không chứa sẵn `__`.
    ///
    /// Một tên như `a__b` mã hoá thành `a____b` rồi giải mã lại thành `a..b` — nghĩa là
    /// hai tool khác nhau có thể va vào cùng một tên trên wire. Sổ đăng ký **từ chối**
    /// những cái tên như vậy ngay lúc đăng ký, vì một va chạm tên là một đường vòng qua
    /// bộ lọc quyền: mô hình gõ tên của tool bị cấm và chạm vào tool được phép, hoặc
    /// ngược lại.
    pub fn round_trips(&self) -> bool {
        !self.0.contains(WIRE_SEPARATOR)
    }
}

impl From<&str> for ToolName {
    fn from(value: &str) -> ToolName {
        ToolName::new(value)
    }
}

impl From<String> for ToolName {
    fn from(value: String) -> ToolName {
        ToolName(value)
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ToolName({})", self.0)
    }
}

/// Ra ngoài dưới dạng chuẩn. Chỗ duy nhất dạng wire được sinh ra là
/// [`crate::schema::ToolSchema`], nơi thật sự nói chuyện với mô hình.
impl Serialize for ToolName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
