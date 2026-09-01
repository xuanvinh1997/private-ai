//! Bảng ngôn ngữ: thêm một ngôn ngữ là thêm **một hàng**.
//!
//! Cả bộ máy — parse, trích, ghi, tra — không biết tên ngôn ngữ nào. Nó chỉ biết ba thứ
//! trong [`Lang`]: đuôi tệp nào thuộc về nó, grammar nào đọc nó, và truy vấn nào lấy ký
//! hiệu ra. Đó là lý do phần khó của việc thêm TypeScript sau này không phải là mã, mà
//! là viết đúng mười dòng truy vấn.
//!
//! Truy vấn dùng một quy ước đặt tên capture, và quy ước đó là toàn bộ hợp đồng giữa tệp
//! này và [`crate::extract`]:
//!
//! - `@name` — nút định danh mang **tên** ký hiệu.
//! - `@def.function` / `@def.type` / `@def.trait` / `@def.const` — nút bao trọn **khối**
//!   khai báo. Phần sau dấu chấm là loại ký hiệu.
//! - `@def.scope` — cũng là một khối, nhưng **không** được phát ra thành ký hiệu; nó chỉ
//!   tồn tại để cho những ký hiệu nằm trong nó một cái tên cha. `impl Foo` của Rust là ví
//!   dụ đúng: `Foo` đã có mặt với tư cách `struct`, kể nó lần thứ hai là nói dối về số
//!   lượng kiểu trong repo, nhưng `fn bar` bên trong thì vẫn phải biết nó thuộc về `Foo`.
//!
//! Quan hệ cha–con **không** được khai trong truy vấn. Nó được suy ra từ bao hàm phạm vi
//! byte, nên nó đúng cho mọi ngôn ngữ mà không ngôn ngữ nào phải nói thêm gì.

use std::path::Path;

use tree_sitter::Language as Grammar;
use tree_sitter_language::LanguageFn;

pub struct Lang {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    grammar: LanguageFn,
    pub query: &'static str,
}

impl Lang {
    /// Dựng `Language` từ con trỏ hàm của grammar.
    ///
    /// Đây là chỗ duy nhất ABI giữa core và grammar gặp nhau, và nó **không** được hệ
    /// kiểu kiểm: một grammar sinh bởi CLI quá cũ sẽ trả về một `Language` mà
    /// `Query::new` từ chối. Vì thế truy vấn được biên dịch hết ngay lúc dựng
    /// [`crate::extract::Extractor`], chứ không lười tới lần parse đầu tiên — một lệch
    /// version phải nổ lúc khởi động, không phải giữa một lần người dùng đang tìm.
    pub fn grammar(&self) -> Grammar {
        Grammar::from(self.grammar)
    }
}

/// Ngôn ngữ nào đọc tệp này. `None` là "không phải mã nguồn mà ta hiểu" — bỏ qua, không
/// phải lỗi.
pub fn for_path(path: &Path) -> Option<&'static Lang> {
    let ext = path.extension()?.to_str()?;
    LANGUAGES.iter().find(|lang| lang.extensions.contains(&ext))
}

pub static LANGUAGES: &[Lang] = &[
    Lang {
        name: "rust",
        extensions: &["rs"],
        grammar: tree_sitter_rust::LANGUAGE,
        query: RUST,
    },
    Lang {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        query: TYPESCRIPT,
    },
    Lang {
        name: "tsx",
        extensions: &["tsx"],
        grammar: tree_sitter_typescript::LANGUAGE_TSX,
        query: TYPESCRIPT,
    },
    Lang {
        name: "javascript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        grammar: tree_sitter_javascript::LANGUAGE,
        query: JAVASCRIPT,
    },
    Lang {
        name: "python",
        extensions: &["py", "pyi"],
        grammar: tree_sitter_python::LANGUAGE,
        query: PYTHON,
    },
];

/// `impl` và `mod` là scope chứ không phải ký hiệu — xem ghi chú đầu tệp.
const RUST: &str = r#"
(function_item name: (identifier) @name) @def.function
(function_signature_item name: (identifier) @name) @def.function
(macro_definition name: (identifier) @name) @def.function

(struct_item name: (type_identifier) @name) @def.type
(enum_item name: (type_identifier) @name) @def.type
(union_item name: (type_identifier) @name) @def.type
(type_item name: (type_identifier) @name) @def.type

(trait_item name: (type_identifier) @name) @def.trait

(const_item name: (identifier) @name) @def.const
(static_item name: (identifier) @name) @def.const

(mod_item name: (identifier) @name) @def.scope
(impl_item type: (type_identifier) @name) @def.scope
(impl_item type: (generic_type type: (type_identifier) @name)) @def.scope
"#;

/// Hằng bị neo vào `program` hoặc `export_statement`, còn hàm thì không.
///
/// Không neo thì `const x = 1` trong thân mỗi hàm cũng thành một ký hiệu, và chỉ mục biến
/// thành một danh sách biến cục bộ mà không ai tra. Hàm thì ngược lại — một
/// `function_declaration` lồng trong một hàm khác vẫn là thứ người ta đi tìm.
const TYPESCRIPT: &str = r#"
(function_declaration name: (identifier) @name) @def.function
(generator_function_declaration name: (identifier) @name) @def.function
(method_definition name: (property_identifier) @name) @def.function

(class_declaration name: (type_identifier) @name) @def.type
(abstract_class_declaration name: (type_identifier) @name) @def.type
(type_alias_declaration name: (type_identifier) @name) @def.type
(enum_declaration name: (identifier) @name) @def.type

(interface_declaration name: (type_identifier) @name) @def.trait

(program
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [(arrow_function) (function_expression)])) @def.function)
(export_statement
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [(arrow_function) (function_expression)])) @def.function)

(program
  (lexical_declaration (variable_declarator name: (identifier) @name)) @def.const)
(export_statement
  (lexical_declaration (variable_declarator name: (identifier) @name)) @def.const)
"#;

const JAVASCRIPT: &str = r#"
(function_declaration name: (identifier) @name) @def.function
(generator_function_declaration name: (identifier) @name) @def.function
(method_definition name: (property_identifier) @name) @def.function

(class_declaration name: (identifier) @name) @def.type

(program
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [(arrow_function) (function_expression)])) @def.function)
(export_statement
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name
      value: [(arrow_function) (function_expression)])) @def.function)

(program
  (lexical_declaration (variable_declarator name: (identifier) @name)) @def.const)
(export_statement
  (lexical_declaration (variable_declarator name: (identifier) @name)) @def.const)
"#;

const PYTHON: &str = r#"
(function_definition name: (identifier) @name) @def.function
(class_definition name: (identifier) @name) @def.type
(module
  (expression_statement
    (assignment left: (identifier) @name)) @def.const)
"#;
