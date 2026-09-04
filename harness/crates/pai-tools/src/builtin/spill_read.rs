//! `spill_read` — read back what was cut from a tool result.
//! Without it, "the full text is in the store" is an empty promise, since the ticket lives
//! in `meta`. The parameters mirror `read` so there is no second pagination style to learn.

use async_trait::async_trait;
use pai_core::Context;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::budget::Overflow;
use crate::name::ToolName;
use crate::schema::{ToolMeta, ToolSchema, json_schema_for};
use crate::seam::Spill;
use crate::tool::{Invocation, Tool, ToolError, ToolOutcome};

/// Lines returned per call by default; the same number as `read`.
const DEFAULT_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SpillReadArgs {
    /// Mã vé, lấy từ dòng "toàn văn … trong kho" của một kết quả đã bị cắt.
    pub id: String,
    /// Dòng bắt đầu, đếm từ 1. Bỏ trống là đọc từ đầu.
    pub offset: Option<usize>,
    /// Số dòng tối đa. Bỏ trống là 2000.
    pub limit: Option<usize>,
}

pub struct SpillRead {
    ctx: Context,
    overflow: Overflow,
}

impl SpillRead {
    pub const NAME: &'static str = "spill_read";

    pub fn new(ctx: &Context) -> SpillRead {
        SpillRead {
            ctx: ctx.clone(),
            overflow: Overflow::new(ctx),
        }
    }
}

#[async_trait]
impl Tool for SpillRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            SpillRead::NAME,
            "Đọc lại toàn văn của một kết quả tool đã bị cắt bớt cho vừa ngân sách. Mã vé \
             nằm trong chính thông báo cắt. Kết quả có đánh số dòng, và phân trang bằng \
             `offset`/`limit` y như `read`.",
            json_schema_for::<SpillReadArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // The store holds other tools' output, so it is no more trustworthy than whatever produced it.
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: SpillReadArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let store = self
            .ctx
            .get::<Spill>()
            .ok_or_else(|| ToolError::Failed("phiên này không có kho tràn nào".into()))?;
        let full = store.read_id(&args.id).ok_or_else(|| {
            // An expired ticket is normal, since the store is session-scoped, so name the way out.
            ToolError::Invalid(format!(
                "không còn vé `{}` trong kho; hãy chạy lại tool đã sinh ra nó.",
                args.id
            ))
        })?;

        let all: Vec<&str> = full.lines().collect();
        let total = all.len();
        let start = args.offset.unwrap_or(1).max(1) - 1;
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).max(1);

        let mut rendered = String::new();
        for (offset, line) in all.iter().skip(start).take(limit).enumerate() {
            rendered.push_str(&format!("{:>6}\t{line}\n", start + offset + 1));
        }
        if rendered.is_empty() {
            rendered = format!("(vé có {total} dòng; không có dòng nào trong khoảng đã hỏi)\n");
        }

        // `spill_read` is budgeted too: reading a 200k-line ticket whole just moves the overflow.
        let name = ToolName::new(SpillRead::NAME);
        let folded = self.overflow.fold(&name, rendered, |split| {
            format!(
                "Đọc tiếp bằng `spill_read` với `id: \"{}\"`, `offset: {}`.",
                args.id,
                start + split.head_lines + 1
            )
        });

        let mut outcome = ToolOutcome::ok(folded.content).with_meta(
            "spill_source",
            json!({ "id": args.id, "offset": start + 1, "total_lines": total }),
        );
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
        Ok(outcome)
    }
}
