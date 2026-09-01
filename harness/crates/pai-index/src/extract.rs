//! Từ cây cú pháp ra ký hiệu.
//!
//! Hai quyết định định hình tệp này.
//!
//! **Truy vấn được biên dịch một lần, lúc dựng.** Một truy vấn hỏng, hay một grammar
//! lệch ABI, là lỗi cấu hình chứ không phải lỗi dữ liệu — nó phải nổ lúc khởi động, một
//! lần, chứ không phải mỗi lần quét lại nuốt lặng một ngôn ngữ.
//!
//! **Quan hệ cha–con suy từ bao hàm, không khai trong truy vấn.** Một truy vấn khai được
//! quan hệ đó phải nhắc lại nó cho từng cặp nút của từng ngôn ngữ; bao hàm phạm vi byte
//! thì đúng cho mọi ngôn ngữ và không ai phải viết thêm gì. Cái giá là một lần sắp xếp
//! và một cái ngăn xếp, dưới đây.

use std::collections::HashMap;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

use crate::lang::{LANGUAGES, Lang};
use crate::symbol::{Symbol, SymbolKind};

#[derive(Debug, thiserror::Error)]
#[error("truy vấn của ngôn ngữ `{lang}` không biên dịch được: {source}")]
pub struct QueryBuildError {
    pub lang: &'static str,
    #[source]
    pub source: tree_sitter::QueryError,
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

struct Compiled {
    lang: &'static Lang,
    query: Query,
    /// Chỉ số capture → vai. Tra bằng chỉ số chứ không bằng chuỗi: so chuỗi cho mọi
    /// capture của mọi match là cách biến việc trích thành việc so sánh chuỗi.
    roles: HashMap<u32, Role>,
    name_capture: Option<u32>,
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
    /// Biên dịch truy vấn của **mọi** ngôn ngữ trong bảng.
    pub fn new() -> Result<Extractor, QueryBuildError> {
        let mut langs = Vec::with_capacity(LANGUAGES.len());
        for lang in LANGUAGES {
            let query =
                Query::new(&lang.grammar(), lang.query).map_err(|source| QueryBuildError {
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
            langs.push(Compiled {
                lang,
                query,
                roles,
                name_capture,
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

    /// Trích ký hiệu từ một tệp đã đọc.
    ///
    /// Không trả `Result`: tree-sitter luôn dựng được một cây, kể cả từ mã hỏng — chỗ
    /// hỏng thành nút `ERROR` và phần còn lại vẫn phân tích được. Một tệp hỏng vì thế trả
    /// về **ít ký hiệu hơn**, không phải một lỗi làm gãy cả lần quét. Đó chính là hành vi
    /// mong muốn khi quét một repo đang có người sửa dở.
    pub fn extract(&self, lang: &'static Lang, path: &str, source: &str) -> Vec<Symbol> {
        let Some(compiled) = self.compiled(lang) else {
            return Vec::new();
        };
        let Some(name_capture) = compiled.name_capture else {
            return Vec::new();
        };

        let mut parser = Parser::new();
        if parser.set_language(&lang.grammar()).is_err() {
            tracing::error!(lang = lang.name, "grammar không nạp được vào parser");
            return Vec::new();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Vec::new();
        };

        let mut found: HashMap<(usize, usize), Hit> = HashMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&compiled.query, tree.root_node(), source.as_bytes());
        while let Some(item) = matches.next() {
            let mut name: Option<&str> = None;
            let mut def: Option<(Role, tree_sitter::Node)> = None;
            for capture in item.captures {
                if capture.index == name_capture {
                    name = capture.node.utf8_text(source.as_bytes()).ok();
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

        let lines: Vec<&str> = source.lines().collect();
        let mut stack: Vec<&Hit> = Vec::new();
        let mut symbols = Vec::new();
        for hit in &hits {
            while stack
                .last()
                .is_some_and(|top| top.end_byte <= hit.start_byte)
            {
                stack.pop();
            }
            if let Role::Symbol(kind) = hit.role {
                symbols.push(Symbol {
                    name: hit.name.clone(),
                    kind,
                    path: path.to_string(),
                    start_line: hit.start_row as u32 + 1,
                    end_line: hit.end_row as u32 + 1,
                    parent: stack.last().map(|top| top.name.clone()),
                    signature: signature(&lines, hit.start_row),
                });
            }
            stack.push(hit);
        }
        symbols
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
