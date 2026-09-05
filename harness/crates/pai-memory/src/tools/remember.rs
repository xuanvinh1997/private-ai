//! `memory.remember` — write entities, observations and edges in one idempotent call.

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::graph::{EntityInput, RelationInput};
use crate::tools::{SharedGraph, cancelled};

/// A batch bigger than this is a model dumping a transcript into memory rather than distilling
/// it; refusing is kinder than storing noise it will have to read back forever.
const MAX_ITEMS: usize = 100;
/// Ceiling on observations across the whole call. `MAX_ITEMS` bounds the outer lists only, and
/// one entity carrying ten thousand sentences is the same transcript dump by another route — it
/// would also mean ten thousand inserts inside the one transaction that holds the lock.
const MAX_OBSERVATIONS_PER_CALL: usize = 500;
/// Ceiling on one observation. The schema asks for a self-contained sentence; something this long
/// is a document, and a document belongs in the project's files, not in a fact the model has to
/// re-read on every search.
const MAX_OBSERVATION_BYTES: usize = 2_000;
/// Ceiling on a name, a kind, or a verb. These are looked up by exact match, so a name too long
/// to retype is a name nobody can read back — and a hundred of them would blow the render budget
/// before a single observation was printed.
const MAX_LABEL_BYTES: usize = 200;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberEntity {
    /// Tên của thực thể. Đây chính là danh tính: ghi lại cùng một tên là cập nhật, không phải tạo mới.
    pub name: String,
    /// Loại của thực thể, ví dụ `người`, `dự án`, `công ty`, `sở thích`. Bỏ trống khi cập nhật thì giữ nguyên loại cũ.
    #[serde(default)]
    pub kind: Option<String>,
    /// Các câu sự thật về thực thể này, mỗi câu một ý và tự đứng được một mình. Câu trùng sẽ bị bỏ qua.
    #[serde(default)]
    pub observations: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberRelation {
    /// Tên thực thể ở đầu mũi tên.
    pub from: String,
    /// Động từ mô tả quan hệ, viết ở thể chủ động, ví dụ `làm việc tại`.
    pub verb: String,
    /// Tên thực thể ở cuối mũi tên.
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberArgs {
    /// Các thực thể cần ghi hoặc bổ sung quan sát.
    #[serde(default)]
    pub entities: Vec<RememberEntity>,
    /// Các quan hệ cần ghi. Cả hai đầu phải là thực thể đã có, hoặc được khai ngay trong `entities` của cùng lần gọi này.
    #[serde(default)]
    pub relations: Vec<RememberRelation>,
}

pub struct MemoryRemember {
    graph: SharedGraph,
}

impl MemoryRemember {
    pub const NAME: &'static str = "memory.remember";

    pub fn new(graph: SharedGraph) -> MemoryRemember {
        MemoryRemember { graph }
    }
}

#[async_trait]
impl Tool for MemoryRemember {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            MemoryRemember::NAME,
            "Ghi vào trí nhớ dài hạn: thực thể, quan sát về thực thể, và quan hệ giữa chúng. \
             Gọi khi biết được điều đáng nhớ qua nhiều phiên làm việc — con người, sở thích, \
             quyết định, ràng buộc của dự án. Ghi lại cùng một điều lần nữa là an toàn: trùng \
             thì bị bỏ qua, không nhân đôi. Quan hệ chỉ được ghi khi cả hai đầu đã tồn tại hoặc \
             được khai trong cùng lần gọi.",
            json_schema_for::<RememberArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Writes to a file on disk that outlives the session, so `mutating` is the honest answer.
        // Nothing leaves the machine. Not concurrency-safe: two parallel `remember` calls would
        // queue on one SQLite write lock anyway, and serialising them here keeps the wait out of
        // the connection.
        ToolMeta::mutating().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: RememberArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        if args.entities.is_empty() && args.relations.is_empty() {
            return Err(ToolError::Invalid(
                "Cần ít nhất một thực thể hoặc một quan hệ để ghi.".to_string(),
            ));
        }
        if args.entities.len() > MAX_ITEMS || args.relations.len() > MAX_ITEMS {
            return Err(ToolError::Invalid(format!(
                "Mỗi lần gọi ghi tối đa {MAX_ITEMS} thực thể và {MAX_ITEMS} quan hệ; hãy chia nhỏ."
            )));
        }
        let observations_total: usize = args
            .entities
            .iter()
            .map(|entity| entity.observations.len())
            .sum();
        if observations_total > MAX_OBSERVATIONS_PER_CALL {
            return Err(ToolError::Invalid(format!(
                "Mỗi lần gọi ghi tối đa {MAX_OBSERVATIONS_PER_CALL} câu quan sát (lần này {observations_total}); hãy chia nhỏ."
            )));
        }
        // Refusing beats silently trimming: half a fact stored as if it were whole is a lie the
        // model will read back for months.
        if let Some(long) = args
            .entities
            .iter()
            .flat_map(|entity| entity.observations.iter())
            .find(|body| body.len() > MAX_OBSERVATION_BYTES)
        {
            return Err(ToolError::Invalid(format!(
                "Có câu quan sát dài {} byte, trần {MAX_OBSERVATION_BYTES}; hãy tách thành nhiều câu ngắn, mỗi câu một ý.",
                long.len()
            )));
        }
        let long_label = args
            .entities
            .iter()
            .flat_map(|entity| [Some(&entity.name), entity.kind.as_ref()])
            .chain(
                args.relations
                    .iter()
                    .flat_map(|edge| [Some(&edge.from), Some(&edge.verb), Some(&edge.to)]),
            )
            .flatten()
            .find(|label| label.len() > MAX_LABEL_BYTES);
        if let Some(long) = long_label {
            return Err(ToolError::Invalid(format!(
                "Tên/loại/động từ dài {} byte, trần {MAX_LABEL_BYTES}; hãy đặt tên ngắn gọn và đưa phần còn lại vào `observations`.",
                long.len()
            )));
        }
        if cancelled(call) {
            return Err(ToolError::Failed("Lệnh đã bị huỷ.".to_string()));
        }

        let entities: Vec<EntityInput> = args
            .entities
            .into_iter()
            .map(|entity| EntityInput {
                name: entity.name,
                kind: entity.kind.unwrap_or_default(),
                observations: entity.observations,
            })
            .collect();
        let relations: Vec<RelationInput> = args
            .relations
            .into_iter()
            .map(|edge| RelationInput {
                from: edge.from,
                verb: edge.verb,
                to: edge.to,
            })
            .collect();

        // One transaction, held under the lock. The batch is capped at 100 items, so the critical
        // section is bounded and there is nothing long-running to interleave a cancel check with.
        let report = {
            let mut graph = self.graph.lock();
            graph
                .remember(&entities, &relations)
                .map_err(|err| ToolError::Failed(err.to_string()))?
        };

        let mut text = format!(
            "Đã ghi: {} thực thể mới, {} thực thể cập nhật, {} quan sát mới, {} quan hệ mới.",
            report.entities_created,
            report.entities_updated,
            report.observations_added,
            report.relations_created
        );
        if !report.skipped_relations.is_empty() {
            // Naming what was skipped lets the model fix it in one follow-up call instead of
            // discovering the missing edge later by reading the graph back.
            let listed = report
                .skipped_relations
                .iter()
                .map(|edge| format!("{} --{}--> {}", edge.from, edge.verb, edge.to))
                .collect::<Vec<_>>()
                .join("; ");
            text.push_str(&format!(
                "\nBỏ qua {} quan hệ vì thiếu thực thể ở một đầu: {listed}. \
                 Hãy khai các thực thể đó rồi ghi lại quan hệ.",
                report.skipped_relations.len()
            ));
        }

        let structured = serde_json::to_value(&report).unwrap_or_else(|_| json!({}));
        Ok(ToolOutcome::ok(text).with_structured(structured))
    }
}
