//! The index vocabulary: a symbol, and four symbol kinds.

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

/// An extracted symbol; `path` is absolute and resolved, exactly what `read` takes and what the model copies on.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    /// 1-based, to match what the editor shows.
    pub start_line: u32,
    pub end_line: u32,
    /// The enclosing symbol's name, if any. One level only — see [`Symbol::qualified`].
    pub parent: Option<String>,
    /// The truncated declaration line; what the model uses to decide whether a `read` is worth it.
    pub signature: String,
}

impl Symbol {
    /// `Foo::bar` rather than `bar`; one parent level only, since the grandparent is already in the file path.
    pub fn qualified(&self) -> String {
        match &self.parent {
            Some(parent) => format!("{parent}::{}", self.name),
            None => self.name.clone(),
        }
    }
}
