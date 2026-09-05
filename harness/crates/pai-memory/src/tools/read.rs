//! `memory.read` — read named entities, or a recent slice of the whole graph.

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{MAX_OBSERVATIONS, SharedGraph, cancelled, render};

/// Default slice of the whole graph. Small on purpose: the whole-graph read exists for orientation
/// at the start of a session, not for bulk transfer.
const DEFAULT_LIMIT: usize = 25;
/// Hard cap, and there is no way to ask for "everything". The MCP server's `read_graph` returns
/// the entire file, which is exactly the failure this crate was written to avoid.
const MAX_LIMIT: usize = 100;
/// Ceiling on how many names one call may ask for. Not politeness: every name becomes a bound
/// parameter in one `IN (...)`, and SQLite refuses a statement past its variable limit — so an
/// uncapped list turns a large-but-legal request into a raw SQLite error the model cannot read.
const MAX_NAMES: usize = MAX_LIMIT;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Tên chính xác của những thực thể cần đọc. Bỏ trống thì đọc lướt cả đồ thị, ưu tiên thứ vừa được cập nhật.
    #[serde(default)]
    pub names: Vec<String>,
    /// Tối đa bao nhiêu thực thể. Mặc định 25, trần 100.
    pub limit: Option<usize>,
}

pub struct MemoryRead {
    graph: SharedGraph,
}

impl MemoryRead {
    pub const NAME: &'static str = "memory.read";

    pub fn new(graph: SharedGraph) -> MemoryRead {
        MemoryRead { graph }
    }
}

#[async_trait]
impl Tool for MemoryRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            MemoryRead::NAME,
            "Đọc trí nhớ dài hạn. Đưa `names` để lấy đúng những thực thể đó cùng quan hệ giữa \
             chúng; bỏ trống `names` để xem lướt những thực thể vừa được cập nhật gần đây. \
             Khi chỉ nhớ mang máng thì dùng `memory.search`, đừng đọc cả đồ thị.",
            json_schema_for::<ReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Read-only, and trusted for the same reason as `memory.search` — see the note there.
        ToolMeta::read_only().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ReadArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let names: Vec<String> = args
            .names
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        if names.len() > MAX_NAMES {
            return Err(ToolError::Invalid(format!(
                "Mỗi lần đọc tối đa {MAX_NAMES} tên; hãy chia nhỏ."
            )));
        }
        // Asking for N names and getting back `DEFAULT_LIMIT` of them silently is a trap: the
        // model would read the answer as "the rest do not exist". When names are given they set
        // the floor, and `MAX_NAMES` keeps that floor under the hard cap.
        let limit = args
            .limit
            .unwrap_or(DEFAULT_LIMIT)
            .max(names.len())
            .clamp(1, MAX_LIMIT);
        if cancelled(call) {
            return Err(ToolError::Failed("Lệnh đã bị huỷ.".to_string()));
        }

        let (entities, relations, stats) = {
            let graph = self.graph.lock();
            let filter = if names.is_empty() {
                None
            } else {
                Some(names.as_slice())
            };
            let entities = graph
                .entities(filter, limit, MAX_OBSERVATIONS)
                .map_err(|err| ToolError::Failed(err.to_string()))?;
            let ids: Vec<i64> = entities.iter().map(|entity| entity.id).collect();
            let relations = graph
                .relations_among(&ids)
                .map_err(|err| ToolError::Failed(err.to_string()))?;
            let stats = graph
                .stats()
                .map_err(|err| ToolError::Failed(err.to_string()))?;
            (entities, relations, stats)
        };

        if entities.is_empty() {
            let text = if names.is_empty() {
                "Trí nhớ đang trống.".to_string()
            } else {
                // Names are the identity here, so a miss is almost always a spelling difference;
                // point at search rather than letting the model conclude the fact is gone.
                format!(
                    "Không tìm thấy thực thể nào tên: {}. Trí nhớ đang có {} thực thể — thử `memory.search` nếu chỉ nhớ mang máng.",
                    names.join(", "),
                    stats.entities
                )
            };
            return Ok(ToolOutcome::ok(text));
        }

        let (mut body, shown) = render(&entities, &relations);
        if shown < entities.len() {
            body.push_str(&format!(
                "\n\n(đã cắt: chỉ hiện {shown}/{} thực thể vì kết quả quá dài)",
                entities.len()
            ));
        }
        // A partial miss is the dangerous one: five names in, four blocks out, and nothing said
        // about the fifth reads as "that fact is gone" rather than "that name is spelled
        // differently". Name the misses so the follow-up is a search, not a rewrite of history.
        // Measured against everything the query found, not against what survived the render:
        // an entity dropped for length is reported by the "đã cắt" line above, and calling it
        // missing here would be the opposite of true.
        let missing: Vec<&str> = names
            .iter()
            .filter(|wanted| !entities.iter().any(|entity| entity.name == **wanted))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            body.push_str(&format!(
                "\n\n(không có trong trí nhớ: {} — thử `memory.search` nếu chỉ nhớ mang máng)",
                missing.join(", ")
            ));
        }
        if names.is_empty() && (stats.entities as usize) > entities.len() {
            body.push_str(&format!(
                "\n\n(đồ thị có {} thực thể, đây chỉ là {} thực thể mới cập nhật nhất)",
                stats.entities,
                entities.len()
            ));
        }

        // Only what the text actually shows. `structured` is not free: `pai-mcp`'s expose layer
        // forwards it as `structured_content`, so anything left out of the render but left in here
        // is a second copy of the payload that walks straight past [`MAX_CHARS`].
        let structured = json!({
            "shown": shown,
            "stats": stats,
            "missing": missing,
            "entities": &entities[..shown],
            "relations": relations,
        });
        Ok(ToolOutcome::ok(body).with_structured(structured))
    }
}
