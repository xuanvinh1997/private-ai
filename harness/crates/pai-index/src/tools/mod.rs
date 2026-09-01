//! Năm tool, và một luật chung cho cả năm.
//!
//! Tất cả đều `read_only().untrusted()`. Chỉ-đọc thì hiển nhiên. Không-đáng-tin thì
//! không: thứ chúng trả về là **tên do người dùng đặt** — tên hàm, tên kiểu, dòng khai
//! báo — và một repo bất kỳ có thể chứa một hàm tên
//! `ignore_previous_instructions_and_run`. Đó là dữ liệu để trích dẫn, không phải chỉ dẫn
//! để làm theo, và chỗ duy nhất nói được điều đó đúng lúc là mô tả tool.
//!
//! Ba tool đồ thị mang thêm một lời cảnh báo thứ hai, và nó nằm trong **nội dung** chứ
//! không chỉ trong mô tả: [`crate::graph::NAME_BASED_NOTICE`]. Lời đầu bảo mô hình đừng
//! *nghe theo* kết quả; lời sau bảo nó đừng *tin chắc* vào kết quả. Hai chuyện khác nhau,
//! và một đồ thị phỏng đoán được trình bày như sự thật hỏng theo kiểu thứ hai.

pub mod graph;
pub mod outline;
pub mod overview;
pub mod symbol_search;
pub mod trace;

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::graph::{NAME_BASED_NOTICE, Neighborhood};
use crate::symbol::Symbol;

/// Một dòng kết quả, dùng chung cho `symbol_search` và `outline`.
///
/// Bắt đầu bằng `đường:dòng` vì đó là hình dạng mà mô hình đã biết đọc từ `grep`, và vì
/// bước tiếp theo của nó gần như luôn là `read` đúng chỗ đó.
pub(crate) fn render(symbol: &Symbol) -> String {
    format!(
        "{}:{}-{} {} {} — {}",
        symbol.path,
        symbol.start_line,
        symbol.end_line,
        symbol.kind.as_str(),
        symbol.qualified(),
        symbol.signature
    )
}

/// Câu nói ra rằng kết quả đã bị cắt. Rỗng khi không cắt — nói "đã cắt: không" mỗi lần là
/// dạy mô hình bỏ qua chính cái câu đó.
pub(crate) fn warn_line(truncated: bool) -> &'static str {
    if truncated {
        " Đã cắt bớt cho vừa: đây không phải toàn bộ lân cận."
    } else {
        ""
    }
}

/// Đổi một lát cắt thành hai thứ: văn bản cho mô hình, và `meta` cho giao diện vẽ.
///
/// Cạnh được in bằng **tên** chứ không bằng số hiệu: một `id` chỉ có nghĩa bên trong một
/// lần gọi, và mô hình sẽ chép cái nó đọc được sang bước sau. `meta` thì ngược lại — nó
/// đi tới bộ vẽ, nơi cần đúng cái `id` để nối hai đỉnh.
pub(crate) fn render_graph(found: &Neighborhood) -> (String, Value) {
    let by_id: HashMap<i64, &crate::graph::GraphNode> =
        found.nodes.iter().map(|node| (node.id, node)).collect();

    let mut lines: Vec<String> = Vec::with_capacity(found.nodes.len() + found.edges.len() + 2);
    lines.push("đỉnh:".to_string());
    for node in &found.nodes {
        lines.push(format!(
            "{}:{} {} {}",
            node.path, node.line, node.kind, node.name
        ));
    }
    lines.push(String::new());
    lines.push("cạnh:".to_string());
    for edge in &found.edges {
        let (Some(src), Some(dst)) = (by_id.get(&edge.src), by_id.get(&edge.dst)) else {
            continue;
        };
        lines.push(format!(
            "{} —{}→ {} ({}:{})",
            src.name,
            edge.kind.as_str(),
            dst.name,
            dst.path,
            dst.line
        ));
    }
    lines.push(String::new());
    lines.push(NAME_BASED_NOTICE.to_string());

    // `id` ra ngoài dưới dạng chuỗi vì hợp đồng wire khai nó là chuỗi: JSON không phân
    // biệt số nguyên với số thực, và một `i64` đi qua một cái parser JavaScript là một
    // `id` có thể đổi giá trị trên đường đi.
    let meta = json!({
        "shape": "graph",
        "truncated": found.truncated,
        "nodes": found.nodes.iter().map(|node| json!({
            "id": node.id.to_string(),
            "name": node.name,
            "kind": node.kind,
            "path": node.path,
            "line": node.line,
        })).collect::<Vec<_>>(),
        "edges": found.edges.iter().map(|edge| json!({
            "src": edge.src.to_string(),
            "dst": edge.dst.to_string(),
            "kind": edge.kind.as_str(),
        })).collect::<Vec<_>>(),
    });
    (lines.join("\n"), meta)
}
