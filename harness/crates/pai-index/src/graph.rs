//! Graph vocabulary, and what an edge does not promise.
//! Only `Contains` is a syntactic fact; the other five are name-based guesses, so every
//! ambiguous name keeps all its candidates and every tool result carries the notice below.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Must appear in every tool result carrying edges — in the payload, not just the tool description.
pub const NAME_BASED_NOTICE: &str = "Cạnh `calls`, `imports`, `implements`, `extends` và \
`references` là suy đoán theo tên, không phải phân tích kiểu: một tên trùng nhau ở nhiều \
nơi sinh ra nhiều cạnh, và một lời gọi qua biến hay qua trait object có thể không sinh \
cạnh nào. Chỉ `contains` là chắc chắn. Kiểm lại bằng `read` trước khi dựa vào nó để sửa mã.";

/// The `kind` label of a whole-file node; deliberately not a [`crate::SymbolKind`], since the model filters on those four.
pub const MODULE_KIND: &str = "module";

/// Six relations, exactly the wire contract. There is no seventh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// A call inside a symbol's body.
    Calls,
    /// `use` / `import` / `require` / `from ... import`.
    Imports,
    /// Parent contains child. The only kind that is not a guess.
    Contains,
    /// `impl Trait for T`, `class A implements I`.
    Implements,
    /// `class A extends B`, `class A(B)`.
    Extends,
    /// A type name in a signature: parameter, return type, annotation.
    References,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Contains => "contains",
            EdgeKind::Implements => "implements",
            EdgeKind::Extends => "extends",
            EdgeKind::References => "references",
        }
    }

    pub fn parse(text: &str) -> Option<EdgeKind> {
        match text {
            "calls" => Some(EdgeKind::Calls),
            "imports" => Some(EdgeKind::Imports),
            "contains" => Some(EdgeKind::Contains),
            "implements" => Some(EdgeKind::Implements),
            "extends" => Some(EdgeKind::Extends),
            "references" => Some(EdgeKind::References),
            _ => None,
        }
    }

    /// Whether this edge is a syntactic fact. See the header.
    pub fn is_structural(self) -> bool {
        matches!(self, EdgeKind::Contains)
    }

    /// Whether a module node may be this edge's target: `import os` names a file, `os.path()` never does.
    pub fn may_target_module(self) -> bool {
        matches!(self, EdgeKind::Imports | EdgeKind::Contains)
    }
}

/// A node; `kind` is a string because [`MODULE_KIND`] is not one of the four symbol kinds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct GraphEdge {
    pub src: i64,
    pub dst: i64,
    pub kind: EdgeKind,
}

/// A slice around one symbol, trimmed to fit a screen and a context window.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Neighborhood {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Trimmed by a depth or node/edge cap; said out loud, or "cut short" looks like "nothing more".
    pub truncated: bool,
}

/// A directory and its contents: the "module" half of the architecture map.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DirectorySummary {
    pub path: String,
    pub files: u32,
    pub symbols: u32,
}

/// A most-connected symbol — the first thing worth reading in an unfamiliar repo.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CentralSymbol {
    pub node: GraphNode,
    pub incoming: u32,
    pub outgoing: u32,
}

impl CentralSymbol {
    pub fn degree(&self) -> u32 {
        self.incoming + self.outgoing
    }
}

/// The architecture map: read before reading code.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Overview {
    pub directories: Vec<DirectorySummary>,
    /// `(language, file count)`, most first.
    pub languages: Vec<(String, u32)>,
    pub central: Vec<CentralSymbol>,
    /// How many directories were cut from `directories`.
    pub directories_omitted: u32,
}

/// Index health; maps straight onto the wire contract's `IndexStats`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub files: u32,
    /// Excludes module nodes: this counts declarations, so it stays comparable across scans.
    pub symbols: u32,
    pub edges: u32,
    /// `(language, file count)`, most first.
    pub languages: Vec<(String, u32)>,
    /// Last scan, epoch milliseconds.
    pub scanned_at: Option<i64>,
}

/// Who owns a reference; three variants rather than one name, because their certainty differs.
#[derive(Clone, Debug, PartialEq)]
pub enum Owner {
    /// An index into the just-extracted `Vec<Symbol>`. Exact.
    Symbol(usize),
    /// A `@def.scope` such as `impl Foo`: not a symbol itself, so the name is looked up within this file.
    Scope(String),
    /// File level: a top-of-file `use` sits in no symbol, so the module node owns it.
    File,
}

/// A reference's target.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// An index into the just-extracted `Vec<Symbol>` — only `contains` is this sure.
    Symbol(usize),
    /// A name awaiting resolution: where the graph stops being fact and becomes a guess.
    Name(String),
}

/// A relation just seen in the syntax tree, not yet resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub from: Owner,
    pub to: Target,
    pub kind: EdgeKind,
    /// The mention's line, 1-based.
    pub line: u32,
}
