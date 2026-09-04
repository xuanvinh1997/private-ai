//! Five tools, all `read_only().untrusted()`.
//! Untrusted because they return user-authored names, which are data to quote, not orders.
//! The three graph tools also carry [`crate::graph::NAME_BASED_NOTICE`] in their payload.

pub mod graph;
pub mod outline;
pub mod overview;
pub mod symbol_search;
pub mod trace;

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::graph::{NAME_BASED_NOTICE, Neighborhood};
use crate::symbol::Symbol;

/// One result line, shared by `symbol_search` and `outline`; starts `path:line` like `grep` output.
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

/// Says the result was cut; empty otherwise, since a constant "not cut" teaches the model to skip the line.
pub(crate) fn warn_line(truncated: bool) -> &'static str {
    if truncated {
        " Đã cắt bớt cho vừa: đây không phải toàn bộ lân cận."
    } else {
        ""
    }
}

/// Render a slice twice: text by name for the model, `meta` by id for the UI renderer.
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

    // `id` goes out as a string: JSON has no integer type, and an `i64` through a JS parser can change value.
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
