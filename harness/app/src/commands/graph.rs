//! Đồ thị bộ nhớ mã nguồn, cho màn hình duyệt.

use pai_index::{GraphNode, Index};
use tauri::State;

use crate::AppState;
use crate::protocol::{GraphEdgeView, GraphNodeView, GraphView, IndexStats};

/// Chỉ mục của dự án đang mở.
///
/// Vắng mặt là trạng thái **hợp lệ**, không phải lỗi: dự án tài liệu không cắm `index`, và
/// một hộp lỗi ở đó chỉ nói với người dùng rằng có gì đó hỏng trong khi mọi thứ đúng như
/// thiết kế. Câu trả lời nói ra loại dự án, vì đó là thứ họ cần biết để hiểu.
fn index(
    harness: &crate::harness::Harness,
) -> Result<std::sync::Arc<dyn pai_index::SymbolIndex>, String> {
    harness
        .ctx
        .get::<Index>()
        .ok_or_else(|| "dự án đang mở không có chỉ mục mã nguồn".to_string())
}

fn node(node: GraphNode) -> GraphNodeView {
    GraphNodeView {
        id: node.id.to_string(),
        name: node.name,
        kind: node.kind,
        path: node.path,
        line: node.line,
    }
}

#[tauri::command]
pub async fn index_stats(state: State<'_, AppState>) -> Result<IndexStats, String> {
    let harness = state.harness().await?;
    let stats = index(&harness)?.stats().await.map_err(|e| e.to_string())?;
    Ok(IndexStats {
        files: stats.files,
        symbols: stats.symbols,
        edges: stats.edges,
        languages: stats.languages,
        scanned_at: stats.scanned_at,
    })
}

#[tauri::command]
pub async fn graph_neighborhood(
    symbol: String,
    depth: Option<u32>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<GraphView, String> {
    let harness = state.harness().await?;
    let found = index(&harness)?
        .neighborhood(&symbol, depth.unwrap_or(1), limit.unwrap_or(60))
        .await
        .map_err(|err| err.to_string())?;
    Ok(GraphView {
        nodes: found.nodes.into_iter().map(node).collect(),
        edges: found
            .edges
            .into_iter()
            .map(|edge| GraphEdgeView {
                src: edge.src.to_string(),
                dst: edge.dst.to_string(),
                kind: edge.kind.as_str().to_string(),
            })
            .collect(),
        truncated: found.truncated,
    })
}

/// Ai gọi ký hiệu này, hoặc nó gọi ai.
///
/// Lõi trả về **các đường đi** (`Vec<Vec<GraphNode>>`), còn giao diện vẽ một đồ thị — nên
/// ở đây các đường được gấp lại thành đỉnh và cạnh. Gấp như vậy làm mất thông tin: hai
/// đường khác nhau đi qua cùng một cặp đỉnh trở thành một cạnh. Chấp nhận được vì màn hình
/// vẽ một hình, và một hình có ba cạnh trùng nhau giữa hai đỉnh thì không nói thêm được gì.
/// `kind` để trống chuỗi `calls` cho mọi cạnh gấp ra: chúng đến từ một truy vấn *chỉ* đi
/// theo cạnh gọi, nên nhãn nào khác cũng là bịa.
#[tauri::command]
pub async fn graph_trace(
    symbol: String,
    direction: String,
    depth: Option<u32>,
    state: State<'_, AppState>,
) -> Result<GraphView, String> {
    let harness = state.harness().await?;
    let index = index(&harness)?;
    let depth = depth.unwrap_or(2);
    let paths = match direction.as_str() {
        "callers" => index.callers(&symbol, depth).await,
        "callees" => index.callees(&symbol, depth).await,
        other => return Err(format!("chiều không hợp lệ: `{other}`")),
    }
    .map_err(|err| err.to_string())?;

    let mut nodes: indexmap::IndexMap<i64, GraphNodeView> = indexmap::IndexMap::new();
    let mut edges: std::collections::BTreeSet<(String, String)> = Default::default();
    for path in &paths {
        for pair in path.windows(2) {
            // `callers` trả đường **đi ngược**: phần tử sau gọi phần tử trước. Đảo lại ở
            // đây để cạnh trên màn hình luôn đọc theo một chiều — người gọi trỏ vào người
            // bị gọi — bất kể người dùng đang hỏi chiều nào.
            let (from, to) = if direction == "callers" {
                (pair[1].id, pair[0].id)
            } else {
                (pair[0].id, pair[1].id)
            };
            edges.insert((from.to_string(), to.to_string()));
        }
        for item in path {
            nodes.entry(item.id).or_insert_with(|| node(item.clone()));
        }
    }

    Ok(GraphView {
        nodes: nodes.into_values().collect(),
        edges: edges
            .into_iter()
            .map(|(src, dst)| GraphEdgeView {
                src,
                dst,
                kind: "calls".to_string(),
            })
            .collect(),
        // Lõi đã chặn số đường ở `MAX_PATHS`; chạm trần nghĩa là còn đường chưa hiện.
        truncated: paths.len() >= pai_index::MAX_PATHS,
    })
}
