//! Syntax tree to symbols and relations.
//! Queries compile once at construction, so an ABI or query error fails at startup. Nesting
//! and reference owners both come from byte containment, in one merged byte-ordered walk.

use std::collections::HashMap;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::graph::{EdgeKind, Owner, Reference, Target};
use crate::lang::{LANGUAGES, Lang};
use crate::symbol::{Symbol, SymbolKind};

#[derive(Debug, thiserror::Error)]
#[error("truy vấn của ngôn ngữ `{lang}` không biên dịch được: {source}")]
pub struct QueryBuildError {
    pub lang: &'static str,
    #[source]
    pub source: tree_sitter::QueryError,
}

/// One extracted file: symbols, plus references left unresolved — only the store sees every file at once.
#[derive(Debug, Default, PartialEq)]
pub struct Extraction {
    pub symbols: Vec<Symbol>,
    pub refs: Vec<Reference>,
}

/// The role of a `@def.*` capture.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Symbol(SymbolKind),
    /// Supplies a parent name; never becomes a symbol itself.
    Scope,
}

impl Role {
    /// Tie-break when two patterns capture one node; tree-sitter's match order is not part of its contract.
    fn rank(self) -> u8 {
        match self {
            Role::Symbol(SymbolKind::Function) => 4,
            Role::Symbol(SymbolKind::Trait) => 3,
            Role::Symbol(SymbolKind::Type) => 2,
            Role::Symbol(SymbolKind::Constant) => 1,
            Role::Scope => 0,
        }
    }
}

fn role_of(capture: &str) -> Option<Role> {
    match capture {
        "def.function" => Some(Role::Symbol(SymbolKind::Function)),
        "def.type" => Some(Role::Symbol(SymbolKind::Type)),
        "def.trait" => Some(Role::Symbol(SymbolKind::Trait)),
        "def.const" => Some(Role::Symbol(SymbolKind::Constant)),
        "def.scope" => Some(Role::Scope),
        _ => None,
    }
}

/// `contains` is absent: it comes from the containment stack, not a query pattern — see [`crate::graph::EdgeKind::is_structural`].
fn edge_of(capture: &str) -> Option<EdgeKind> {
    match capture {
        "ref.calls" => Some(EdgeKind::Calls),
        "ref.imports" => Some(EdgeKind::Imports),
        "ref.implements" => Some(EdgeKind::Implements),
        "ref.extends" => Some(EdgeKind::Extends),
        "ref.references" => Some(EdgeKind::References),
        _ => None,
    }
}

struct Compiled {
    lang: &'static Lang,
    query: Query,
    /// Capture index to role; by index, not by string, or extraction becomes string comparison.
    roles: HashMap<u32, Role>,
    name_capture: Option<u32>,
    edges: Query,
    /// Edge-query capture index to relation kind; `_`-prefixed captures exist only for text predicates.
    edge_roles: HashMap<u32, EdgeKind>,
}

/// Reusable across files and threads: `Query` is `Send + Sync`, `Parser` is not, so a parser is built per extraction.
pub struct Extractor {
    langs: Vec<Compiled>,
}

impl Extractor {
    /// Compile both queries of every language in the table.
    pub fn new() -> Result<Extractor, QueryBuildError> {
        let mut langs = Vec::with_capacity(LANGUAGES.len());
        for lang in LANGUAGES {
            let grammar = lang.grammar();
            let query = Query::new(&grammar, lang.query).map_err(|source| QueryBuildError {
                lang: lang.name,
                source,
            })?;
            let mut roles = HashMap::new();
            let mut name_capture = None;
            for (index, capture) in query.capture_names().iter().enumerate() {
                let index = index as u32;
                if *capture == "name" {
                    name_capture = Some(index);
                } else if let Some(role) = role_of(capture) {
                    roles.insert(index, role);
                }
            }
            let edges = Query::new(&grammar, lang.edges).map_err(|source| QueryBuildError {
                lang: lang.name,
                source,
            })?;
            let mut edge_roles = HashMap::new();
            for (index, capture) in edges.capture_names().iter().enumerate() {
                if let Some(kind) = edge_of(capture) {
                    edge_roles.insert(index as u32, kind);
                }
            }
            langs.push(Compiled {
                lang,
                query,
                roles,
                name_capture,
                edges,
                edge_roles,
            });
        }
        Ok(Extractor { langs })
    }

    fn compiled(&self, lang: &'static Lang) -> Option<&Compiled> {
        // Compare by pointer: the table is `static`, and names could collide later.
        self.langs
            .iter()
            .find(|compiled| std::ptr::eq(compiled.lang, lang))
    }

    /// Extract symbols and relations from one file; broken source yields fewer symbols, never an error.
    pub fn extract(&self, lang: &'static Lang, path: &str, source: &str) -> Extraction {
        let Some(compiled) = self.compiled(lang) else {
            return Extraction::default();
        };
        let Some(name_capture) = compiled.name_capture else {
            return Extraction::default();
        };

        let mut parser = Parser::new();
        if parser.set_language(&lang.grammar()).is_err() {
            tracing::error!(lang = lang.name, "grammar failed to load into the parser");
            return Extraction::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Extraction::default();
        };
        let bytes = source.as_bytes();

        let mut found: HashMap<(usize, usize), Hit> = HashMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&compiled.query, tree.root_node(), bytes);
        while let Some(item) = matches.next() {
            let mut name: Option<&str> = None;
            let mut def: Option<(Role, tree_sitter::Node)> = None;
            for capture in item.captures {
                if capture.index == name_capture {
                    name = capture.node.utf8_text(bytes).ok();
                } else if let Some(role) = compiled.roles.get(&capture.index) {
                    def = Some((*role, capture.node));
                }
            }
            let (Some(name), Some((role, node))) = (name, def) else {
                continue;
            };
            let hit = Hit {
                role,
                name: name.to_string(),
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                start_row: node.start_position().row,
                end_row: node.end_position().row,
            };
            found
                .entry((hit.start_byte, hit.end_byte))
                .and_modify(|existing| {
                    if hit.role.rank() > existing.role.rank() {
                        *existing = hit.clone();
                    }
                })
                .or_insert(hit);
        }

        let mut hits: Vec<Hit> = found.into_values().collect();
        // Outer before inner: the stack below depends on this order.
        hits.sort_by(|a, b| {
            a.start_byte
                .cmp(&b.start_byte)
                .then(b.end_byte.cmp(&a.end_byte))
        });

        let mut mentions: Vec<Mention> = Vec::new();
        let mut edge_cursor = QueryCursor::new();
        let mut edge_matches = edge_cursor.matches(&compiled.edges, tree.root_node(), bytes);
        while let Some(item) = edge_matches.next() {
            for capture in item.captures {
                let Some(kind) = compiled.edge_roles.get(&capture.index) else {
                    continue;
                };
                let Ok(name) = capture.node.utf8_text(bytes) else {
                    continue;
                };
                mentions.push(Mention {
                    kind: *kind,
                    name: name.to_string(),
                    start_byte: capture.node.start_byte(),
                    start_row: capture.node.start_position().row,
                });
            }
        }
        // One node can match two patterns; a duplicate edge costs a row and a resolution for nothing.
        mentions.sort_by(|a, b| {
            a.start_byte
                .cmp(&b.start_byte)
                .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
                .then_with(|| a.name.cmp(&b.name))
        });
        mentions.dedup_by(|a, b| a.start_byte == b.start_byte && a.kind == b.kind);

        let lines: Vec<&str> = source.lines().collect();
        let mut walk = Walk {
            stack: Vec::new(),
            symbol_of_hit: vec![None; hits.len()],
            out: Extraction::default(),
        };
        // Merge declarations and references by byte; on a tie the declaration goes first, or its references lose their owner.
        let mut declarations = hits.iter().enumerate().peekable();
        let mut references = mentions.iter().peekable();
        loop {
            let take_declaration = match (declarations.peek(), references.peek()) {
                (Some((_, hit)), Some(mention)) => hit.start_byte <= mention.start_byte,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_declaration {
                if let Some((index, hit)) = declarations.next() {
                    walk.enter(index, hit, &hits, path, &lines);
                }
            } else if let Some(mention) = references.next() {
                walk.mention(mention, &hits);
            }
        }
        walk.out
    }
}

/// State of the merged walk: the containment stack and what it produces.
struct Walk {
    /// Indices of the open `Hit`s, outermost first.
    stack: Vec<usize>,
    /// Which symbol each `Hit` became; `None` marks a `@def.scope`.
    symbol_of_hit: Vec<Option<usize>>,
    out: Extraction,
}

impl Walk {
    fn close_until(&mut self, start_byte: usize, hits: &[Hit]) {
        while self
            .stack
            .last()
            .is_some_and(|top| hits[*top].end_byte <= start_byte)
        {
            self.stack.pop();
        }
    }

    /// The current owner; an empty stack means file level — see [`Owner::File`].
    fn owner(&self, hits: &[Hit]) -> Owner {
        match self.stack.last() {
            None => Owner::File,
            Some(top) => match self.symbol_of_hit[*top] {
                Some(index) => Owner::Symbol(index),
                None => Owner::Scope(hits[*top].name.clone()),
            },
        }
    }

    fn enter(&mut self, index: usize, hit: &Hit, hits: &[Hit], path: &str, lines: &[&str]) {
        self.close_until(hit.start_byte, hits);
        if let Role::Symbol(kind) = hit.role {
            let owner = self.owner(hits);
            let position = self.out.symbols.len();
            self.out.symbols.push(Symbol {
                name: hit.name.clone(),
                kind,
                path: path.to_string(),
                start_line: hit.start_row as u32 + 1,
                end_line: hit.end_row as u32 + 1,
                parent: self.stack.last().map(|top| hits[*top].name.clone()),
                signature: signature(lines, hit.start_row),
            });
            // The only edge sure of both ends: the target is the symbol just built, not a name to look up.
            self.out.refs.push(Reference {
                from: owner,
                to: Target::Symbol(position),
                kind: EdgeKind::Contains,
                line: hit.start_row as u32 + 1,
            });
            self.symbol_of_hit[index] = Some(position);
        }
        self.stack.push(index);
    }

    fn mention(&mut self, mention: &Mention, hits: &[Hit]) {
        self.close_until(mention.start_byte, hits);
        self.out.refs.push(Reference {
            from: self.owner(hits),
            to: Target::Name(mention.name.clone()),
            kind: mention.kind,
            line: mention.start_row as u32 + 1,
        });
    }
}

/// The declaration line, truncated by characters rather than bytes so a multi-byte character is never split.
fn signature(lines: &[&str], row: usize) -> String {
    const CAP: usize = 160;
    let raw = lines.get(row).copied().unwrap_or_default().trim();
    if raw.chars().count() <= CAP {
        return raw.to_string();
    }
    raw.chars().take(CAP).collect::<String>() + "…"
}

#[derive(Clone)]
struct Hit {
    role: Role,
    name: String,
    start_byte: usize,
    end_byte: usize,
    start_row: usize,
    end_row: usize,
}

/// A name mentioned somewhere, with neither owner nor target known yet.
struct Mention {
    kind: EdgeKind,
    name: String,
    start_byte: usize,
    start_row: usize,
}
