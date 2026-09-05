//! `memory.forget` — delete entities, single observations, or single edges.

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::graph::{ObservationTarget, RelationInput};
use crate::tools::{SharedGraph, cancelled};

/// Same ceiling as `memory.remember`: a delete list longer than this is a mistake, and a mistake
/// that deletes is worse than one that writes.
const MAX_ITEMS: usize = 100;
/// And the same ceiling on the nested lists, for the same reason `memory.remember` has one:
/// `MAX_ITEMS` bounds the number of entities named, not the number of sentences hanging off them,
/// so without this one target could carry an unbounded run of deletes inside the transaction.
const MAX_OBSERVATIONS_PER_CALL: usize = 500;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetObservations {
    /// Tên thực thể đang giữ các quan sát này.
    pub entity: String,
    /// Các câu quan sát cần xoá, phải trùng đúng từng chữ với câu đã ghi.
    pub observations: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetRelation {
    /// Tên thực thể ở đầu mũi tên.
    pub from: String,
    /// Động từ của quan hệ cần xoá.
    pub verb: String,
    /// Tên thực thể ở cuối mũi tên.
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetArgs {
    /// Tên các thực thể cần xoá hẳn. Xoá thực thể thì mọi quan sát và mọi quan hệ chạm vào nó cũng mất theo.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Chỉ xoá vài câu quan sát, giữ nguyên thực thể.
    #[serde(default)]
    pub observations: Vec<ForgetObservations>,
    /// Chỉ xoá vài quan hệ, giữ nguyên hai thực thể ở hai đầu.
    #[serde(default)]
    pub relations: Vec<ForgetRelation>,
}

pub struct MemoryForget {
    graph: SharedGraph,
}

impl MemoryForget {
    pub const NAME: &'static str = "memory.forget";

    pub fn new(graph: SharedGraph) -> MemoryForget {
        MemoryForget { graph }
    }
}

#[async_trait]
impl Tool for MemoryForget {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            MemoryForget::NAME,
            "Xoá khỏi trí nhớ dài hạn. Dùng khi một điều đã ghi trở nên sai, hết hạn, hoặc \
             người dùng bảo quên đi. Xoá một thực thể sẽ kéo theo toàn bộ quan sát và quan hệ \
             của nó — muốn giữ thực thể thì chỉ xoá quan sát hoặc quan hệ. Việc xoá không hoàn tác được.",
            json_schema_for::<ForgetArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Destructive and durable: `mutating`, and serialised for the same reason as `remember`.
        ToolMeta::mutating().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ForgetArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        if args.entities.is_empty() && args.observations.is_empty() && args.relations.is_empty() {
            return Err(ToolError::Invalid(
                "Cần nói rõ cần quên cái gì: `entities`, `observations`, hoặc `relations`."
                    .to_string(),
            ));
        }
        if args.entities.len() > MAX_ITEMS
            || args.observations.len() > MAX_ITEMS
            || args.relations.len() > MAX_ITEMS
        {
            return Err(ToolError::Invalid(format!(
                "Mỗi lần gọi xoá tối đa {MAX_ITEMS} mục mỗi loại; hãy chia nhỏ."
            )));
        }
        let observations_total: usize = args
            .observations
            .iter()
            .map(|target| target.observations.len())
            .sum();
        if observations_total > MAX_OBSERVATIONS_PER_CALL {
            return Err(ToolError::Invalid(format!(
                "Mỗi lần gọi xoá tối đa {MAX_OBSERVATIONS_PER_CALL} câu quan sát (lần này {observations_total}); hãy chia nhỏ."
            )));
        }
        if cancelled(call) {
            return Err(ToolError::Failed("Lệnh đã bị huỷ.".to_string()));
        }

        let observations: Vec<ObservationTarget> = args
            .observations
            .into_iter()
            .map(|target| ObservationTarget {
                entity: target.entity,
                observations: target.observations,
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

        let report = {
            let mut graph = self.graph.lock();
            graph
                .forget(&args.entities, &observations, &relations)
                .map_err(|err| ToolError::Failed(err.to_string()))?
        };

        // The counts are the whole answer: "0 entities" is how the model learns it misspelled a
        // name, rather than believing a delete happened.
        let text = format!(
            "Đã quên: {} thực thể, {} quan sát, {} quan hệ.",
            report.entities, report.observations, report.relations
        );
        let structured = serde_json::to_value(&report).unwrap_or_else(|_| json!({}));
        Ok(ToolOutcome::ok(text).with_structured(structured))
    }
}
