//! Language table: adding a language means adding one row.
//! Nothing downstream knows a language name, only extensions, grammar and two queries.
//! Capture names are the contract with `extract`; nesting comes from byte-range containment.

use std::path::Path;

use tree_sitter::Language as Grammar;
use tree_sitter_language::LanguageFn;

pub struct Lang {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    grammar: LanguageFn,
    /// The symbol query.
    pub query: &'static str,
    /// The edge query; empty is valid, so a new language is searchable before its edges are written.
    pub edges: &'static str,
}

impl Lang {
    /// Build a `Language` from the grammar function pointer; the core/grammar ABI is unchecked, so queries compile up front.
    pub fn grammar(&self) -> Grammar {
        Grammar::from(self.grammar)
    }
}

/// Which language reads this file; `None` means "not source we understand" — skip it, not an error.
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

/// `impl` and `mod` are scopes, not symbols — see the header.
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

/// Constants are anchored to `program`/`export_statement`, functions are not: otherwise every local `const` becomes a symbol.
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

/// `use` is captured at the last name of the path only: the earlier segments resolve to nothing in the symbol table.
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

/// `implements`/`extends` are separate patterns, not nested in `class_heritage`: an interface uses `extends_type_clause`.
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

/// JavaScript's `class_heritage` holds the expression directly, and `require` needs a text predicate or every call looks like an import.
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

/// `import_from_statement` has two `dotted_name` fields; only `name:` enters scope, `module_name:` is a path.
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
