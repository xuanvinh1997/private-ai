//! Định dạng của một tài liệu.
//!
//! Danh sách này phải khớp **ba chỗ**: `Format::as_str` ở đây, nhãn `format` mà
//! `services/rag/src/pai_rag_service/extract/__init__.py` trả về, và union
//! `DocumentFormat` trong `ui/src/lib/protocol.ts`. Lệch một chỗ thì hoặc bảng tài liệu
//! hiện một ô trống, hoặc `Format::parse` lặng lẽ gom một định dạng lạ vào `Text`.
//!
//! Nhóm theo **cách đọc**, không theo phần mở rộng: `office` là mọi thứ markitdown mở
//! bằng đường OOXML — `.docx`, `.xlsx`, `.pptx` — và tách chúng thành ba nhãn chỉ để bảng
//! hiện ba chữ khác nhau là ba nhánh phải giữ đồng bộ mà không đổi được hành vi nào.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Pdf,
    /// `.docx`, `.xlsx`, `.pptx` và họ hàng — đọc qua markitdown.
    Office,
    /// Ảnh, đọc bằng mô hình vision. Chỉ có mặt khi người dùng đã chọn một model vision.
    Image,
    Html,
    /// `.csv`, `.tsv`, `.json`, `.xml`, `.yaml` — có cấu trúc nhưng đọc ra như văn bản.
    Data,
    Markdown,
    Code,
    Text,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Office => "office",
            Format::Image => "image",
            Format::Html => "html",
            Format::Data => "data",
            Format::Markdown => "markdown",
            Format::Code => "code",
            Format::Text => "text",
        }
    }

    /// Từ chuỗi trên dây.
    ///
    /// Định dạng lạ rơi về [`Format::Text`] thay vì thành lỗi: một nhãn mới bên Python
    /// không được phép làm cả danh sách tài liệu không hiện ra. `docx` được nhận cho
    /// tương thích với kho đã ghi bằng bản trước.
    pub fn parse(name: &str) -> Format {
        match name.trim().to_ascii_lowercase().as_str() {
            "pdf" => Format::Pdf,
            "office" | "docx" | "xlsx" | "pptx" => Format::Office,
            "image" => Format::Image,
            "html" => Format::Html,
            "data" | "csv" => Format::Data,
            "markdown" => Format::Markdown,
            "code" => Format::Code,
            _ => Format::Text,
        }
    }
}
