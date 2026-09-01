//! Từ cây cú pháp ra ký hiệu **và** quan hệ.
//!
//! Ba quyết định định hình tệp này.
//!
//! **Truy vấn được biên dịch một lần, lúc dựng.** Một truy vấn hỏng, hay một grammar
//! lệch ABI, là lỗi cấu hình chứ không phải lỗi dữ liệu — nó phải nổ lúc khởi động, một
//! lần, chứ không phải mỗi lần quét lại nuốt lặng một ngôn ngữ.
//!
//! **Quan hệ cha–con suy từ bao hàm, không khai trong truy vấn.** Một truy vấn khai được
//! quan hệ đó phải nhắc lại nó cho từng cặp nút của từng ngôn ngữ; bao hàm phạm vi byte
//! thì đúng cho mọi ngôn ngữ và không ai phải viết thêm gì. Cái giá là một lần sắp xếp
//! và một cái ngăn xếp, dưới đây.
//!
//! **Chủ nhà của một tham chiếu đi qua đúng cái ngăn xếp ấy.** Một lời gọi không tự nói
//! nó nằm trong hàm nào; thứ nói ra điều đó là việc nút gọi nằm lọt trong phạm vi byte
//! của một khai báo. Vì thế khai báo và tham chiếu được trộn vào **một** lần duyệt theo
//! thứ tự byte: hai lần duyệt riêng sẽ phải dựng lại cùng một ngăn xếp hai lần và có hai
//! chỗ để lệch nhau.

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

/// Một tệp đã trích xong: ký hiệu, và những quan hệ **chưa** phân giải.
///
/// Tham chiếu chưa phân giải ở đây chứ không ở tầng trên vì tệp này không nhìn thấy tệp
/// khác — mà `helper()` trong tệp này thì hay được khai ở tệp kia. Phân giải là việc của
/// [`crate::store::Store::rebuild_edges`], nơi cả kho cùng có mặt một lúc.
#[derive(Debug, Default, PartialEq)]
pub struct Extraction {
    pub symbols: Vec<Symbol>,
    pub refs: Vec<Reference>,
}

/// Vai của một capture `@def.*`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Symbol(SymbolKind),
    /// Cho tên cha, không tự mình thành ký hiệu.
    Scope,
}

impl Role {
    /// Xếp hạng khi hai mẫu cùng bắt trúng một khối.
    ///
    /// `export const f = () => {}` khớp cả mẫu hàm lẫn mẫu hằng, và cả hai trỏ vào đúng
    /// một nút. Chọn theo thứ tự mẫu trong tệp truy vấn thì kết quả phụ thuộc vào thứ tự
    /// match mà tree-sitter trả về — một thứ không có trong hợp đồng của nó. Một thang
    /// hạng tường minh thì luôn ra cùng một đáp án.
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

/// `contains` không có ở đây: nó là sự thật của cái ngăn xếp bao hàm, không phải của một
/// mẫu truy vấn — xem [`crate::graph::EdgeKind::is_structural`].
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
    /// Chỉ số capture → vai. Tra bằng chỉ số chứ không bằng chuỗi: so chuỗi cho mọi
    /// capture của mọi match là cách biến việc trích thành việc so sánh chuỗi.
    roles: HashMap<u32, Role>,
    name_capture: Option<u32>,
    edges: Query,
    /// Chỉ số capture của truy vấn cạnh → loại quan hệ. Capture bắt đầu bằng `_` không có
    /// mặt ở đây: chúng chỉ tồn tại để một vị từ văn bản có cái mà so.
    edge_roles: HashMap<u32, EdgeKind>,
}

/// Bộ trích, dùng lại được cho nhiều tệp và nhiều luồng.
///
/// `Query` là `Send + Sync`, `Parser` thì không — nên bộ trích giữ truy vấn còn parser
/// được dựng tại chỗ trong mỗi lần trích. Dựng một `Parser` là cấp phát vài trăm byte;
/// biên dịch lại một `Query` thì không.
pub struct Extractor {
    langs: Vec<Compiled>,
}

impl Extractor {
    /// Biên dịch **cả hai** truy vấn của **mọi** ngôn ngữ trong bảng.
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
        // So bằng con trỏ: bảng ngôn ngữ là `static`, nên hai tham chiếu tới cùng một
        // hàng luôn cùng địa chỉ, và tên thì có thể trùng nhau về sau.
        self.langs
            .iter()
            .find(|compiled| std::ptr::eq(compiled.lang, lang))
    }

    /// Trích ký hiệu và quan hệ từ một tệp đã đọc.
    ///
    /// Không trả `Result`: tree-sitter luôn dựng được một cây, kể cả từ mã hỏng — chỗ
    /// hỏng thành nút `ERROR` và phần còn lại vẫn phân tích được. Một tệp hỏng vì thế trả
    /// về **ít ký hiệu hơn**, không phải một lỗi làm gãy cả lần quét. Đó chính là hành vi
    /// mong muốn khi quét một repo đang có người sửa dở.
    pub fn extract(&self, lang: &'static Lang, path: &str, source: &str) -> Extraction {
        let Some(compiled) = self.compiled(lang) else {
            return Extraction::default();
        };
        let Some(name_capture) = compiled.name_capture else {
            return Extraction::default();
        };

        let mut parser = Parser::new();
        if parser.set_language(&lang.grammar()).is_err() {
            tracing::error!(lang = lang.name, "grammar không nạp được vào parser");
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
        // Ngoài trước, trong sau: đó là điều kiện để cái ngăn xếp dưới đây đúng.
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
        // Cùng một nút có thể bị hai mẫu bắt trúng; hai cạnh giống hệt nhau không nói thêm
        // gì mà vẫn tốn một hàng và một lần phân giải.
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
        // Trộn khai báo và tham chiếu vào một dòng thời gian theo byte. Khi hai thứ bắt
        // đầu ở cùng một byte thì khai báo đi trước: nó là cái bao ngoài, và một tham
        // chiếu được gán chủ nhà trước khi chủ nhà kịp vào ngăn xếp là một cạnh mất gốc.
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

/// Trạng thái của lần duyệt trộn: ngăn xếp bao hàm, và cái nó sinh ra.
struct Walk {
    /// Chỉ số của những `Hit` đang mở, ngoài dưới cùng.
    stack: Vec<usize>,
    /// `Hit` nào đã thành ký hiệu thứ mấy. `None` là một `@def.scope`.
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

    /// Chủ nhà hiện tại. Ngăn xếp rỗng nghĩa là tầng tệp — xem [`Owner::File`].
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
            // Cạnh duy nhất biết chắc cả hai đầu: đích là chính ký hiệu vừa dựng, không
            // phải một cái tên phải đi tra.
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

/// Dòng khai báo, cắt ngắn.
///
/// Cắt theo ký tự chứ không theo byte: cắt giữa một ký tự nhiều byte sinh ra chuỗi không
/// phải UTF-8, và tên định danh tiếng Việt trong comment là chuyện bình thường ở repo này.
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

/// Một cái tên được nhắc tới ở đâu đó, chưa biết của ai và trỏ vào đâu.
struct Mention {
    kind: EdgeKind,
    name: String,
    start_byte: usize,
    start_row: usize,
}
