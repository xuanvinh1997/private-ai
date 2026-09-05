//! `think` — a place to take a problem apart step by step, and to revise a step already taken.
//! Replaces the `server-sequential-thinking` subprocess. Closest relative in this crate is
//! `todo_write`: session state behind a `Mutex`, nothing on disk, nothing on the wire.
//!
//! What was cut from the original server, and why:
//!   * `isRevision` / `needsMoreThoughts` — two booleans restating `revisedThought` and
//!     `nextThoughtNeeded`; a flag that can disagree with the field beside it is a bug waiting
//!     to be filed, so the optional fields alone carry the meaning;
//!   * the chalk-coloured boxes it printed to stderr — the host draws the UI here, from
//!     `structured`, and a tool that formats a terminal is a tool guessing where it runs.
//!
//! What was added: a chain resets when step 1 arrives outside a revision or a branch. The
//! original keeps one ever-growing history for the life of the process, which makes "step 3 of
//! 5" a lie the moment a session's second problem starts.

use async_trait::async_trait;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::schema::{ToolMeta, ToolSchema, json_schema_for};
use crate::tool::{Invocation, Tool, ToolError, ToolOutcome};

/// Hard ceiling on one chain. Past a hundred steps the model is circling, not reasoning, and
/// the honest move is to make it stop and answer rather than let it fill the window.
const MAX_STEPS: usize = 100;

/// Longest step text kept. A step is a step; anything longer is a draft answer that belongs in
/// the reply, and keeping it whole would let one chain pin megabytes for the session.
const MAX_STEP_BYTES: usize = 4_000;

/// How many steps the returned summary lists. Twelve lines is enough to see where the chain is
/// without re-reading the whole chain on every call — the earlier steps are already upstream in
/// the conversation, and paying for them twice is what makes this pattern expensive.
const RECENT_STEPS: usize = 12;

/// Characters of a step shown in that list; a label, not the content.
const EXCERPT_CHARS: usize = 72;

/// Longest branch name kept. `branch` is the one caller-supplied string that gets printed back
/// whole, so without a ceiling here the summary is as long as the model cares to make it —
/// and the name is also pinned in the chain for the session. A branch name is a handle the
/// model reuses to mean "the same branch"; anything past a short phrase is prose.
const MAX_BRANCH_CHARS: usize = 48;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThinkArgs {
    /// Nội dung bước suy nghĩ này: một bước thôi, không phải cả lời giải.
    pub thought: String,
    /// Đây là bước thứ mấy, đếm từ 1. Gửi bước 1 là bắt đầu một chuỗi mới.
    pub step: usize,
    /// Ước lượng hiện tại về tổng số bước. Được phép ước lượng lại ở mỗi lần gọi.
    pub total_steps: usize,
    /// Còn cần nghĩ thêm bước nữa hay không. Đặt `false` ở bước cuối cùng.
    pub more_needed: bool,
    /// Nếu bước này viết lại một bước đã ghi, điền số thứ tự của bước đó.
    pub revises: Option<usize>,
    /// Nếu bước này rẽ sang một hướng khác, điền số thứ tự của bước làm gốc rẽ nhánh.
    pub branch_from: Option<usize>,
    /// Tên nhánh, luôn đi kèm `branch_from`, để phân biệt các hướng đang thử.
    pub branch: Option<String>,
}

/// One recorded step. `Serialize` because the whole chain goes to the UI in `structured`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ThoughtStep {
    pub step: usize,
    pub thought: String,
    /// Which earlier step this one rewrites, if any.
    pub revises: Option<usize>,
    pub branch_from: Option<usize>,
    pub branch: Option<String>,
    /// Whether the text above was cut to fit [`MAX_STEP_BYTES`].
    pub truncated: bool,
}

/// One session's chain of reasoning.
#[derive(Default)]
pub struct Think {
    steps: Mutex<Vec<ThoughtStep>>,
}

impl Think {
    pub const NAME: &'static str = "think";

    pub fn new() -> Think {
        Think::default()
    }

    /// A snapshot for the UI.
    pub fn snapshot(&self) -> Vec<ThoughtStep> {
        self.steps.lock().clone()
    }

    /// Cut on a character boundary: the text is arbitrary UTF-8 written by the model.
    fn clamp(text: &str) -> (String, bool) {
        if text.len() <= MAX_STEP_BYTES {
            return (text.to_string(), false);
        }
        let end = text
            .char_indices()
            .map(|(at, _)| at)
            .take_while(|at| *at <= MAX_STEP_BYTES)
            .last()
            .unwrap_or(0);
        (text[..end].to_string(), true)
    }

    /// A branch name as it will be stored and reprinted: blank means "the model left the field
    /// in without filling it", which is absent, and an overlong one is cut with a visible `…`
    /// so the summary never inherits the caller's idea of how long a label may be.
    fn label(name: Option<&str>) -> Option<String> {
        let name = name.map(str::trim).filter(|name| !name.is_empty())?;
        Some(match name.char_indices().nth(MAX_BRANCH_CHARS) {
            Some((at, _)) => format!("{}…", &name[..at]),
            None => name.to_string(),
        })
    }

    fn excerpt(step: &ThoughtStep) -> String {
        let flat = step
            .thought
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match flat.char_indices().nth(EXCERPT_CHARS) {
            Some((at, _)) => format!("{}…", &flat[..at]),
            None => flat,
        }
    }

    /// What comes back to the model: where the chain stands, not what it just said.
    ///
    /// Echoing the step verbatim would double its token cost for nothing — the model wrote it,
    /// it is already in the transcript one message above. What it cannot reconstruct for free is
    /// the shape of the chain: how many steps stand, which of them were rewritten, which branches
    /// are open. So that is what this returns, bounded by [`RECENT_STEPS`].
    fn render(steps: &[ThoughtStep], total: usize, more_needed: bool, reset: bool) -> String {
        let Some(current) = steps.last() else {
            return "Chưa ghi được bước nào.".to_string();
        };

        let revisions = steps.iter().filter(|s| s.revises.is_some()).count();
        let mut branches: Vec<&str> = steps
            .iter()
            .filter_map(|s| s.branch.as_deref())
            .collect::<Vec<_>>();
        branches.sort_unstable();
        branches.dedup();

        let mut out = String::new();
        if reset {
            out.push_str("Bắt đầu chuỗi suy nghĩ mới.\n");
        }
        out.push_str(&format!(
            "Đã ghi bước {}/{} · {}",
            current.step,
            total,
            if more_needed {
                "còn nghĩ tiếp"
            } else {
                "đây là bước cuối, hãy trả lời"
            },
        ));
        if let Some(revised) = current.revises {
            out.push_str(&format!(" · viết lại bước {revised}"));
        }
        if let (Some(from), Some(name)) = (current.branch_from, current.branch.as_deref()) {
            out.push_str(&format!(" · nhánh `{name}` rẽ từ bước {from}"));
        }
        if current.truncated {
            out.push_str(&format!(" · nội dung đã bị cắt còn {MAX_STEP_BYTES} byte"));
        }
        out.push_str(&format!(
            "\nChuỗi hiện có {} bước, {revisions} lần viết lại, {} nhánh.",
            steps.len(),
            branches.len(),
        ));

        let skipped = steps.len().saturating_sub(RECENT_STEPS);
        if skipped > 0 {
            out.push_str(&format!("\n  … {skipped} bước đầu không liệt kê"));
        }
        for step in steps.iter().skip(skipped) {
            let mark = if step.revises.is_some() { '*' } else { ' ' };
            out.push_str(&format!(
                "\n  {}{} {}",
                mark,
                step.step,
                Think::excerpt(step)
            ));
        }
        out
    }
}

#[async_trait]
impl Tool for Think {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Think::NAME,
            "Ghi lại một bước suy nghĩ khi bài toán cần tháo ra từng khúc: mỗi lần gọi là một \
             bước, và có thể viết lại một bước cũ (`revises`) hay rẽ sang hướng khác \
             (`branch_from`) khi hướng đang đi tỏ ra sai. `total_steps` chỉ là ước lượng, cứ \
             sửa lại khi thấy rõ hơn. Đặt `more_needed: false` ở bước cuối rồi trả lời. Tool \
             trả về tóm tắt trạng thái chuỗi, không chép lại nội dung vừa gửi.",
            json_schema_for::<ThinkArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Not `mutating`: like `todo_write` it writes only this turn's scratchpad, so a
        // read-only agent may still reason out loud.
        // Not concurrency-safe: steps are numbered, and two writers numbering at once produce a
        // chain that reads as if one of them never happened.
        ToolMeta::read_only().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ThinkArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;

        // Nobody is waiting for this result any more, so recording the step would only corrupt
        // the numbering of whatever runs next.
        if call.cancel_token().is_cancelled() {
            return Err(ToolError::Failed(
                "lượt đã bị huỷ trước khi ghi bước".into(),
            ));
        }

        if args.step == 0 {
            return Err(ToolError::Invalid(
                "`step` đếm từ 1, không có bước 0.".into(),
            ));
        }
        if args.total_steps == 0 {
            return Err(ToolError::Invalid(
                "`total_steps` phải ít nhất là 1; đó là ước lượng, không cần chính xác.".into(),
            ));
        }
        if args.thought.trim().is_empty() {
            return Err(ToolError::Invalid("`thought` không được để trống.".into()));
        }
        // Normalised before the pairing check, so `branch: ""` is reported as the missing name
        // it is rather than recorded as a branch called nothing.
        let branch = Think::label(args.branch.as_deref());
        if args.branch_from.is_some() != branch.is_some() {
            return Err(ToolError::Invalid(
                "`branch_from` và `branch` phải đi cùng nhau: một nhánh cần cả gốc rẽ lẫn tên."
                    .into(),
            ));
        }

        let mut steps = self.steps.lock();

        // Step 1 that neither rewrites nor branches means a new problem; keeping the old chain
        // would leave every later "bước k/n" counting steps that belong to something else.
        let reset = args.step == 1 && args.revises.is_none() && args.branch_from.is_none();
        if reset {
            steps.clear();
        }

        if steps.len() >= MAX_STEPS {
            return Err(ToolError::Invalid(format!(
                "chuỗi đã đủ {MAX_STEPS} bước — đến lúc kết luận bằng những gì đang có, \
                 hoặc bắt đầu lại bằng `step: 1`."
            )));
        }

        // A pointer into a step that was never recorded turns the chain into fiction, so it is
        // refused rather than dropped silently.
        for (field, target) in [("revises", args.revises), ("branch_from", args.branch_from)] {
            if let Some(target) = target
                && !steps.iter().any(|step| step.step == target)
            {
                return Err(ToolError::Invalid(format!(
                    "`{field}: {target}` trỏ vào một bước chưa được ghi; chuỗi hiện có {} bước.",
                    steps.len()
                )));
            }
        }

        let (thought, truncated) = Think::clamp(&args.thought);
        steps.push(ThoughtStep {
            step: args.step,
            thought,
            revises: args.revises,
            branch_from: args.branch_from,
            branch,
            truncated,
        });

        // An estimate below the step just written is not an estimate; raise it rather than
        // reporting "bước 6/4".
        let total = args.total_steps.max(args.step);
        let rendered = Think::render(&steps, total, args.more_needed, reset);
        let structured = json!({
            "steps": &*steps,
            "total_steps": total,
            "more_needed": args.more_needed,
        });
        drop(steps);

        Ok(ToolOutcome::ok(rendered).with_structured(structured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ToolName;

    fn call(args: Value) -> Invocation {
        Invocation::new(
            ToolName::new(Think::NAME),
            "test",
            args.as_object().cloned().unwrap_or_default(),
        )
    }

    /// A plain step, so the tests read as the chain they describe.
    fn step(n: usize, total: usize, text: &str) -> Value {
        json!({ "thought": text, "step": n, "total_steps": total, "more_needed": true })
    }

    #[tokio::test]
    async fn ghi_tung_buoc_va_dem_dung() {
        let think = Think::new();
        think
            .execute(&call(step(1, 3, "đọc đề")))
            .await
            .expect("bước 1");
        let outcome = think
            .execute(&call(step(2, 3, "thử cách A")))
            .await
            .expect("bước 2");
        assert!(outcome.content.contains("bước 2/3"));
        assert_eq!(think.snapshot().len(), 2);
    }

    /// The point of the tool: the result must not be the argument, or every step costs twice.
    #[tokio::test]
    async fn khong_chep_lai_nguyen_van_buoc_vua_gui() {
        let think = Think::new();
        let long = "chi tiết dài dòng ".repeat(40);
        let outcome = think
            .execute(&call(step(1, 2, &long)))
            .await
            .expect("bước 1");
        assert!(!outcome.content.contains(&long));
        assert!(outcome.content.len() < long.len());
    }

    #[tokio::test]
    async fn viet_lai_mot_buoc_da_ghi() {
        let think = Think::new();
        think
            .execute(&call(step(1, 2, "cách A")))
            .await
            .expect("bước 1");
        let outcome = think
            .execute(&call(json!({
                "thought": "cách A sai vì thiếu điều kiện biên",
                "step": 2,
                "total_steps": 3,
                "more_needed": true,
                "revises": 1,
            })))
            .await
            .expect("viết lại bước 1");
        assert!(outcome.content.contains("viết lại bước 1"));
        assert!(outcome.content.contains("1 lần viết lại"));
    }

    #[tokio::test]
    async fn re_nhanh_tu_mot_buoc_cu() {
        let think = Think::new();
        think
            .execute(&call(step(1, 3, "gốc")))
            .await
            .expect("bước 1");
        let outcome = think
            .execute(&call(json!({
                "thought": "thử hướng khác",
                "step": 2,
                "total_steps": 3,
                "more_needed": true,
                "branch_from": 1,
                "branch": "b",
            })))
            .await
            .expect("rẽ nhánh");
        assert!(outcome.content.contains("nhánh `b` rẽ từ bước 1"));
        assert!(outcome.content.contains("1 nhánh"));
    }

    #[tokio::test]
    async fn nhanh_thieu_mot_nua_thi_bao_loi() {
        let think = Think::new();
        let err = think
            .execute(&call(json!({
                "thought": "x",
                "step": 1,
                "total_steps": 1,
                "more_needed": true,
                "branch": "b",
            })))
            .await
            .expect_err("thiếu `branch_from`");
        assert!(matches!(err, ToolError::Invalid(_)));
    }

    /// `branch` is the only caller string reprinted whole, so it needs its own ceiling: the
    /// step text being clamped does not help if the label beside it is a megabyte.
    #[tokio::test]
    async fn ten_nhanh_qua_dai_thi_bi_cat() {
        let think = Think::new();
        think
            .execute(&call(step(1, 2, "gốc")))
            .await
            .expect("bước 1");
        let outcome = think
            .execute(&call(json!({
                "thought": "rẽ nhánh",
                "step": 2,
                "total_steps": 2,
                "more_needed": true,
                "branch_from": 1,
                "branch": "n".repeat(50_000),
            })))
            .await
            .expect("vẫn rẽ được");
        assert!(outcome.content.len() < 1_000, "tóm tắt phải ngắn");
        assert!(
            outcome.content.contains('…'),
            "phải đánh dấu là đã cắt: {}",
            outcome.content
        );
        let stored = think.snapshot();
        let name = stored[1].branch.as_deref().expect("có tên nhánh");
        assert!(name.chars().count() <= MAX_BRANCH_CHARS + 1);
    }

    /// A field left in but not filled is a field not given; saying "thiếu tên nhánh" beats
    /// recording a branch whose name is the empty string.
    #[tokio::test]
    async fn ten_nhanh_rong_bi_coi_nhu_khong_khai() {
        let think = Think::new();
        think
            .execute(&call(step(1, 2, "gốc")))
            .await
            .expect("bước 1");
        let err = think
            .execute(&call(json!({
                "thought": "rẽ nhánh",
                "step": 2,
                "total_steps": 2,
                "more_needed": true,
                "branch_from": 1,
                "branch": "   ",
            })))
            .await
            .expect_err("tên nhánh rỗng thì coi như thiếu");
        assert!(err.to_string().contains("đi cùng nhau"));
        assert_eq!(think.snapshot().len(), 1, "bước hỏng không được ghi");
    }

    #[tokio::test]
    async fn tro_vao_buoc_chua_ton_tai_thi_bao_loi() {
        let think = Think::new();
        think
            .execute(&call(step(1, 2, "gốc")))
            .await
            .expect("bước 1");
        let err = think
            .execute(&call(json!({
                "thought": "sửa bước trong tưởng tượng",
                "step": 2,
                "total_steps": 2,
                "more_needed": true,
                "revises": 9,
            })))
            .await
            .expect_err("bước 9 chưa có");
        assert!(err.to_string().contains("chưa được ghi"));
    }

    #[tokio::test]
    async fn buoc_mot_moi_thi_xoa_chuoi_cu() {
        let think = Think::new();
        think
            .execute(&call(step(1, 2, "bài toán cũ")))
            .await
            .expect("bước 1");
        think
            .execute(&call(step(2, 2, "vẫn bài toán cũ")))
            .await
            .expect("bước 2");
        let outcome = think
            .execute(&call(step(1, 2, "bài toán khác hẳn")))
            .await
            .expect("mở chuỗi mới");
        assert!(outcome.content.contains("chuỗi suy nghĩ mới"));
        assert_eq!(think.snapshot().len(), 1);
    }

    #[tokio::test]
    async fn uoc_luong_thap_hon_buoc_hien_tai_thi_duoc_nang_len() {
        let think = Think::new();
        think.execute(&call(step(1, 2, "a"))).await.expect("bước 1");
        think.execute(&call(step(2, 2, "b"))).await.expect("bước 2");
        let outcome = think.execute(&call(step(3, 2, "c"))).await.expect("bước 3");
        assert!(outcome.content.contains("bước 3/3"), "{}", outcome.content);
    }

    /// The listing is bounded, or a long chain re-sends itself on every call.
    #[tokio::test]
    async fn chuoi_dai_thi_tom_tat_van_ngan() {
        let think = Think::new();
        for n in 1..=30 {
            think
                .execute(&call(step(
                    n,
                    30,
                    &format!("bước số {n} với chút nội dung"),
                )))
                .await
                .expect("ghi được");
        }
        let outcome = think
            .execute(&call(step(31, 40, "gần xong")))
            .await
            .expect("bước 31");
        assert!(outcome.content.contains("bước đầu không liệt kê"));
        assert_eq!(outcome.content.lines().count(), RECENT_STEPS + 3);
    }

    #[tokio::test]
    async fn qua_tran_so_buoc_thi_buoc_phai_ket_luan() {
        let think = Think::new();
        for n in 1..=MAX_STEPS {
            think
                .execute(&call(step(n, MAX_STEPS, "vòng vo")))
                .await
                .expect("còn trong trần");
        }
        let err = think
            .execute(&call(step(MAX_STEPS + 1, MAX_STEPS + 1, "vẫn vòng vo")))
            .await
            .expect_err("đã chạm trần");
        assert!(err.to_string().contains("kết luận"));
    }

    #[tokio::test]
    async fn noi_dung_qua_dai_thi_cat_va_noi_ro_la_da_cat() {
        let think = Think::new();
        let huge = "đ".repeat(MAX_STEP_BYTES);
        let outcome = think
            .execute(&call(step(1, 1, &huge)))
            .await
            .expect("vẫn ghi được");
        assert!(outcome.content.contains("đã bị cắt"));
        let stored = think.snapshot();
        assert!(stored[0].truncated);
        assert!(stored[0].thought.len() <= MAX_STEP_BYTES);
    }

    #[tokio::test]
    async fn thought_rong_thi_bao_loi() {
        let think = Think::new();
        let err = think
            .execute(&call(step(1, 1, "   ")))
            .await
            .expect_err("bước rỗng vô nghĩa");
        assert!(matches!(err, ToolError::Invalid(_)));
    }

    #[tokio::test]
    async fn buoc_khong_va_tong_khong_deu_bi_tu_choi() {
        let think = Think::new();
        assert!(think.execute(&call(step(0, 1, "x"))).await.is_err());
        assert!(think.execute(&call(step(1, 0, "x"))).await.is_err());
    }

    #[tokio::test]
    async fn khong_ghi_gi_khi_luot_da_bi_huy() {
        let think = Think::new();
        let invocation = call(step(1, 1, "x"));
        invocation.cancel_token().cancel();
        assert!(think.execute(&invocation).await.is_err());
        assert!(think.snapshot().is_empty());
    }

    #[tokio::test]
    async fn tool_khong_thay_doi_trang_thai_ben_ngoai() {
        let meta = Think::new().meta();
        assert!(!meta.mutating);
        assert!(!meta.leaves_device);
        assert!(!meta.concurrency_safe);
    }
}
