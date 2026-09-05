//! `memory.search` — the tool called most often, and therefore the one with the hardest ceiling.

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{MAX_OBSERVATIONS, SharedGraph, cancelled, render};

/// Default hit count. Ten entities with a dozen observations each is roughly a page of text —
/// enough to answer "what do I know about X", nowhere near enough to matter to the budget.
const DEFAULT_LIMIT: usize = 10;
/// Hard cap. Asking for more than this is asking to read the graph, and `memory.read` is that.
const MAX_LIMIT: usize = 40;
/// Ceiling on the query itself. A query is a phrase or a question; anything longer is a paragraph
/// pasted in by mistake, and it would become one `LIKE` pattern plus one FTS expression with a
/// quoted term per word — cost with no chance of a useful hit.
const MAX_QUERY_BYTES: usize = 500;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Từ khoá hoặc câu hỏi. Khớp với tên thực thể, loại thực thể, và nội dung các câu quan sát.
    pub query: String,
    /// Tối đa bao nhiêu thực thể. Mặc định 10, trần 40.
    pub limit: Option<usize>,
}

pub struct MemorySearch {
    graph: SharedGraph,
}

impl MemorySearch {
    pub const NAME: &'static str = "memory.search";

    pub fn new(graph: SharedGraph) -> MemorySearch {
        MemorySearch { graph }
    }
}

#[async_trait]
impl Tool for MemorySearch {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            MemorySearch::NAME,
            "Tìm trong trí nhớ dài hạn. Gọi trước khi trả lời bất cứ câu hỏi nào về người dùng, \
             thói quen, dự án hay quyết định cũ của họ. Khớp cả theo tên thực thể lẫn theo nội \
             dung các câu quan sát, nên hỏi bằng một cụm từ cũng được. Kết quả có trần: khi bị \
             cắt, phần trả về nói rõ còn bao nhiêu.",
            json_schema_for::<SearchArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Reads only. Not `untrusted`: everything in this graph was written by this user or by
        // the model on their behalf through `memory.remember` — the same trust level as the
        // conversation itself. Were the graph ever fed from a web page or another person's data,
        // this line would have to become `.untrusted()`.
        ToolMeta::read_only().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: SearchArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Err(ToolError::Invalid(
                "`query` trống; hãy nêu từ khoá cần tìm.".to_string(),
            ));
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err(ToolError::Invalid(format!(
                "`query` quá dài ({} byte, trần {MAX_QUERY_BYTES}); hãy rút lại còn từ khoá chính.",
                query.len()
            )));
        }
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        if cancelled(call) {
            return Err(ToolError::Failed("Lệnh đã bị huỷ.".to_string()));
        }

        let (hits, relations, total) = {
            let graph = self.graph.lock();
            let hits = graph
                .search(&query, limit, MAX_OBSERVATIONS)
                .map_err(|err| ToolError::Failed(err.to_string()))?;
            let ids: Vec<i64> = hits.iter().map(|hit| hit.entity.id).collect();
            let relations = graph
                .relations_among(&ids)
                .map_err(|err| ToolError::Failed(err.to_string()))?;
            let total = graph
                .stats()
                .map_err(|err| ToolError::Failed(err.to_string()))?
                .entities;
            (hits, relations, total)
        };

        if hits.is_empty() {
            // Say how big the graph is: "nothing found" in an empty memory means something quite
            // different from "nothing found" in a memory of two thousand entities.
            return Ok(ToolOutcome::ok(format!(
                "Không có gì khớp `{query}` trong trí nhớ (đang có {total} thực thể)."
            )));
        }

        let entities: Vec<_> = hits.iter().map(|hit| hit.entity.clone()).collect();
        let (mut body, shown) = render(&entities, &relations);
        if shown < entities.len() {
            body.push_str(&format!(
                "\n\n(đã cắt: chỉ hiện {shown}/{} thực thể tìm được vì kết quả quá dài)",
                entities.len()
            ));
        }

        // Only the hits the text actually shows. `structured` is forwarded as MCP
        // `structured_content` by `pai-mcp`'s expose layer, so a hit dropped by the render but
        // kept here would carry the same bytes past [`crate::tools::MAX_CHARS`] anyway.
        let structured = json!({
            "query": query,
            "entities_total": total,
            "shown": shown,
            "hits": hits.iter().take(shown).map(|hit| json!({
                "name": hit.entity.name,
                "kind": hit.entity.kind,
                "observations": hit.entity.observations,
                "observationsTotal": hit.entity.observations_total,
                "matchedBy": hit.matched_by,
            })).collect::<Vec<_>>(),
            "relations": relations,
        });
        Ok(ToolOutcome::ok(body).with_structured(structured))
    }
}
