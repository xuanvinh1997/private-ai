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
//!
//! Mỗi ngôn ngữ có **hai** truy vấn, và truy vấn thứ hai ([`Lang::edges`]) theo một quy
//! ước khác: mỗi capture bắt đúng cái **nút mang tên** của chỗ nhắc tới, còn chủ nhà của
//! nó lại được suy từ bao hàm — cùng một cơ chế, cùng một lý do.
//!
//! - `@ref.calls` — định danh ở vị trí bị gọi.
//! - `@ref.imports` — tên được mang vào phạm vi bởi một lệnh nhập.
//! - `@ref.implements` / `@ref.extends` — cái được cài đặt hoặc được kế thừa.
//! - `@ref.references` — tên một kiểu trong chữ ký: tham số, kiểu trả về, chú thích.
//!
//! `contains` không có mặt ở đây: nó đã nằm sẵn trong cái ngăn xếp bao hàm mà
//! [`crate::extract`] dựng cho ký hiệu, và hỏi lại nó bằng truy vấn là hỏi hai lần cùng
//! một câu để rồi phải chọn tin câu nào.

use std::path::Path;

use tree_sitter::Language as Grammar;
use tree_sitter_language::LanguageFn;

pub struct Lang {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    grammar: LanguageFn,
    /// Truy vấn ký hiệu.
    pub query: &'static str,
    /// Truy vấn cạnh. Rỗng là hợp lệ: một ngôn ngữ mới vào bảng vẫn tra được ký hiệu
    /// trong lúc chưa ai viết xong phần cạnh cho nó.
    pub edges: &'static str,
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
        edges: RUST_EDGES,
    },
    Lang {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        query: TYPESCRIPT,
        edges: TYPESCRIPT_EDGES,
    },
    Lang {
        name: "tsx",
        extensions: &["tsx"],
        grammar: tree_sitter_typescript::LANGUAGE_TSX,
        query: TYPESCRIPT,
        edges: TYPESCRIPT_EDGES,
    },
    Lang {
        name: "javascript",
        extensions: &["js", "mjs", "cjs", "jsx"],
        grammar: tree_sitter_javascript::LANGUAGE,
        query: JAVASCRIPT,
        edges: JAVASCRIPT_EDGES,
    },
    Lang {
        name: "python",
        extensions: &["py", "pyi"],
        grammar: tree_sitter_python::LANGUAGE,
        query: PYTHON,
        edges: PYTHON_EDGES,
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

/// `use` được bắt ở **tên cuối cùng** của đường dẫn, không phải cả đường.
///
/// `use crate::store::Store` mang vào phạm vi đúng một cái tên là `Store`, và đó cũng là
/// cái tên duy nhất tra được trong bảng ký hiệu. Bắt cả `crate` với `store` thì mỗi lệnh
/// `use` sinh ra ba cạnh, hai trong đó không trỏ vào đâu cả.
const RUST_EDGES: &str = r#"
(call_expression function: (identifier) @ref.calls)
(call_expression function: (scoped_identifier name: (identifier) @ref.calls))
(call_expression function: (field_expression field: (field_identifier) @ref.calls))
(call_expression function: (generic_function function: (identifier) @ref.calls))
(macro_invocation macro: (identifier) @ref.calls)

(use_declaration argument: (identifier) @ref.imports)
(use_declaration argument: (scoped_identifier name: (identifier) @ref.imports))
(use_list (identifier) @ref.imports)
(use_list (scoped_identifier name: (identifier) @ref.imports))
(use_as_clause alias: (identifier) @ref.imports)

(impl_item trait: (type_identifier) @ref.implements)
(impl_item trait: (generic_type type: (type_identifier) @ref.implements))
(impl_item trait: (scoped_type_identifier name: (type_identifier) @ref.implements))

(parameter type: (type_identifier) @ref.references)
(parameter type: (reference_type type: (type_identifier) @ref.references))
(parameter type: (generic_type type: (type_identifier) @ref.references))
(function_item return_type: (type_identifier) @ref.references)
(function_item return_type: (reference_type type: (type_identifier) @ref.references))
(function_item return_type: (generic_type type: (type_identifier) @ref.references))
"#;

/// `implements` và `extends` là hai mẫu tách rời chứ không lồng trong `class_heritage`:
/// `interface J extends I` dùng `extends_type_clause` chứ không đi qua `class_heritage`,
/// và chủ nhà của cạnh vẫn được suy từ bao hàm nên không mẫu nào cần nhắc tới cái lớp.
const TYPESCRIPT_EDGES: &str = r#"
(call_expression function: (identifier) @ref.calls)
(call_expression function: (member_expression property: (property_identifier) @ref.calls))
(new_expression constructor: (identifier) @ref.calls)

(import_specifier name: (identifier) @ref.imports)
(namespace_import (identifier) @ref.imports)
(import_clause (identifier) @ref.imports)

(extends_clause value: (identifier) @ref.extends)
(extends_type_clause type: (type_identifier) @ref.extends)
(implements_clause (type_identifier) @ref.implements)

(type_annotation (type_identifier) @ref.references)
(type_annotation (generic_type name: (type_identifier) @ref.references))
"#;

/// `class_heritage` của JavaScript chứa thẳng một biểu thức — không có `extends_clause`
/// như bên TypeScript, nên hai bảng không dùng chung được mẫu đó.
///
/// `require` phải đi kèm một vị từ văn bản: không có nó thì mọi `const x = f(...)` đều
/// thành một lệnh nhập.
const JAVASCRIPT_EDGES: &str = r#"
(call_expression function: (identifier) @ref.calls)
(call_expression function: (member_expression property: (property_identifier) @ref.calls))
(new_expression constructor: (identifier) @ref.calls)

(import_specifier name: (identifier) @ref.imports)
(namespace_import (identifier) @ref.imports)
(import_clause (identifier) @ref.imports)
(variable_declarator
  name: (identifier) @ref.imports
  value: (call_expression function: (identifier) @_goi)
  (#eq? @_goi "require"))

(class_heritage (identifier) @ref.extends)
"#;

/// `import_from_statement` có hai trường cùng kiểu `dotted_name`; chỉ `name:` mới là thứ
/// được mang vào phạm vi, còn `module_name:` là đường dẫn tệp và không tra được.
const PYTHON_EDGES: &str = r#"
(call function: (identifier) @ref.calls)
(call function: (attribute attribute: (identifier) @ref.calls))

(import_statement name: (dotted_name (identifier) @ref.imports))
(import_statement name: (aliased_import alias: (identifier) @ref.imports))
(import_from_statement name: (dotted_name (identifier) @ref.imports))
(import_from_statement name: (aliased_import alias: (identifier) @ref.imports))

(class_definition superclasses: (argument_list (identifier) @ref.extends))
(class_definition superclasses: (argument_list (attribute attribute: (identifier) @ref.extends)))

(typed_parameter type: (type (identifier) @ref.references))
(function_definition return_type: (type (identifier) @ref.references))
"#;
