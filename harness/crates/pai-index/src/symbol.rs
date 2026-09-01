//! Từ vựng của chỉ mục: một ký hiệu, và bốn loại ký hiệu.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Bốn loại, cố ý ít.
///
/// Một bảng phân loại chi tiết hơn — `method` tách khỏi `function`, `enum` tách khỏi
/// `struct` — nghe đúng hơn nhưng làm hỏng chỗ nó được dùng: mô hình phải đoán đúng nhãn
/// để lọc, và đoán trượt thì kết quả rỗng trông y hệt "không có ký hiệu nào như thế".
/// Bốn loại này giữ được ý định của người hỏi mà không có chỗ nào để đoán trượt; quan hệ
/// method–class đã nằm ở [`Symbol::parent`] rồi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// Hàm, phương thức, macro.
    Function,
    /// Struct, class, enum, type alias.
    Type,
    /// Trait, interface.
    Trait,
    /// Hằng và biến ở tầng module.
    Constant,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Type => "type",
            SymbolKind::Trait => "trait",
            SymbolKind::Constant => "constant",
        }
    }

    pub fn parse(text: &str) -> Option<SymbolKind> {
        match text {
            "function" => Some(SymbolKind::Function),
            "type" => Some(SymbolKind::Type),
            "trait" => Some(SymbolKind::Trait),
            "constant" => Some(SymbolKind::Constant),
            _ => None,
        }
    }
}

/// Một ký hiệu đã trích.
///
/// `path` là đường **tuyệt đối đã phân giải**, giống hệt thứ `read` nhận vào. Lưu đường
/// tương đối thì rẻ hơn vài chục byte và đắt hơn một lớp ghép chuỗi ở mọi chỗ đọc; và
/// mô hình sẽ chép thẳng đường dẫn này sang lần gọi `read` tiếp theo.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    /// Đánh số từ 1, để khớp với cái người ta thấy trong trình soạn thảo.
    pub start_line: u32,
    pub end_line: u32,
    /// Tên ký hiệu bao ngoài, nếu có. Chỉ một tầng — xem [`Symbol::qualified`].
    pub parent: Option<String>,
    /// Dòng khai báo, đã cắt. Đây là thứ để mô hình quyết định có đáng `read` hay không.
    pub signature: String,
}

impl Symbol {
    /// `Foo::bar` thay vì `bar`. Chỉ ghép một tầng cha, vì cha của cha đã là thông tin mà
    /// người đọc lấy được từ đường dẫn tệp.
    pub fn qualified(&self) -> String {
        match &self.parent {
            Some(parent) => format!("{parent}::{}", self.name),
            None => self.name.clone(),
        }
    }
}
