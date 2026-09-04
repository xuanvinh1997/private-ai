//! A tool result's budget, measured in tokens rather than lines.
//! Line counts do not track the context window, so the unit here is bytes over four. Head
//! and tail are both kept, and [`Overflow::fold`] requires a resume hint as a parameter.

use pai_core::Context;

use crate::name::ToolName;
use crate::seam::Spill;
use crate::spill::SpillRef;

/// Bytes per approximate token; four suits BPE on Latin text and overestimates Vietnamese, which errs the safe way.
pub const BYTES_PER_TOKEN: usize = 4;

/// Default budget per tool result in approximate tokens (~24 KiB): enough for most real source files.
pub const DEFAULT_TOKEN_BUDGET: usize = 6_000;

/// A text's approximate token count.
pub fn approx_tokens(text: &str) -> usize {
    text.len().div_ceil(BYTES_PER_TOKEN)
}

/// A folded result: the head, the tail, and what sat between them.
#[derive(Debug, PartialEq)]
pub struct Split<'a> {
    pub head: &'a str,
    pub tail: &'a str,
    /// Complete lines in the head, the number a follow-up `offset` builds on; a half-cut line does not count as read.
    pub head_lines: usize,
    pub tail_lines: usize,
    pub total_lines: usize,
    pub omitted_lines: usize,
    pub total_bytes: usize,
}

/// The result after the budget is applied.
pub struct Folded {
    /// The text the model reads, including the resume hint when something was cut.
    pub content: String,
    /// The ticket for the full text; `None` when nothing was cut or no store is mounted.
    pub spill: Option<SpillRef>,
    pub truncated: bool,
    pub omitted_lines: usize,
    pub total_lines: usize,
}

/// Where a tool applies the budget to its own result; it holds a `Context` and asks for the store at call time.
#[derive(Clone)]
pub struct Overflow {
    ctx: Context,
    budget: usize,
}

impl Overflow {
    pub fn new(ctx: &Context) -> Overflow {
        Overflow {
            ctx: ctx.clone(),
            budget: DEFAULT_TOKEN_BUDGET,
        }
    }

    /// The budget, in approximate tokens.
    pub fn with_budget(mut self, tokens: usize) -> Overflow {
        self.budget = tokens.max(1);
        self
    }

    pub fn budget_tokens(&self) -> usize {
        self.budget
    }

    fn budget_bytes(&self) -> usize {
        self.budget.saturating_mul(BYTES_PER_TOKEN)
    }

    /// `None` when it fits; otherwise a head and a tail of half the budget each.
    pub fn split<'a>(&self, full: &'a str) -> Option<Split<'a>> {
        let budget = self.budget_bytes();
        if full.len() <= budget {
            return None;
        }
        let half = (budget / 2).max(1);
        let head = head_slice(full, half);
        let tail = tail_slice(full, half);
        let total_lines = count_lines(full);
        let head_lines = head.matches('\n').count();
        let tail_lines = count_lines(tail);
        Some(Split {
            head,
            tail,
            head_lines,
            tail_lines,
            total_lines,
            omitted_lines: total_lines.saturating_sub(head_lines + tail_lines),
            total_bytes: full.len(),
        })
    }

    /// Store the full text; `None` means no store is mounted on the tree.
    pub fn store(&self, tool: &ToolName, full: &str) -> Option<SpillRef> {
        self.ctx.get::<Spill>().map(|store| store.spill(tool, full))
    }

    /// Fold a result to fit; `resume` must supply the tool's own hint for reading on, and with no store mounted nothing is cut.
    pub fn fold(
        &self,
        tool: &ToolName,
        full: String,
        resume: impl FnOnce(&Split<'_>) -> String,
    ) -> Folded {
        let Some(split) = self.split(&full) else {
            return Folded {
                truncated: false,
                total_lines: count_lines(&full),
                content: full,
                spill: None,
                omitted_lines: 0,
            };
        };
        let Some(handle) = self.store(tool, &full) else {
            tracing::warn!(tool = %tool, "over budget but no spill store is mounted");
            return Folded {
                truncated: false,
                total_lines: split.total_lines,
                content: full,
                spill: None,
                omitted_lines: 0,
            };
        };

        let hint = resume(&split);
        // Report bytes as well as lines: "0 lines omitted" on a 200 KiB single-line file is true but reassuringly wrong.
        let omitted_bytes = split
            .total_bytes
            .saturating_sub(split.head.len())
            .saturating_sub(split.tail.len());
        let content = format!(
            "{}\n[… đã cắt bớt {omitted_bytes} byte ở giữa (~{} token, {} dòng trọn vẹn) \
             cho vừa ngân sách {} token. Toàn văn {} dòng vẫn nguyên vẹn trong kho: gọi \
             `spill_read` với `id: \"{}\"`. {hint}]\n{}",
            split.head.trim_end_matches('\n'),
            omitted_bytes.div_ceil(BYTES_PER_TOKEN),
            split.omitted_lines,
            self.budget,
            handle.lines,
            handle.id,
            split.tail.trim_start_matches('\n'),
        );

        Folded {
            content,
            truncated: true,
            omitted_lines: split.omitted_lines,
            total_lines: split.total_lines,
            spill: Some(handle),
        }
    }
}

/// Line count, including a final line with no `\n`.
fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().count()
}

/// Step back to the nearest character boundary at or below `idx`.
fn floor_boundary(text: &str, mut idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Step forward to the nearest character boundary at or above `idx`.
fn ceil_boundary(text: &str, mut idx: usize) -> usize {
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx.min(text.len())
}

/// The head, cut at a line end unless that discards over half the allowance, which handles very long single lines.
fn head_slice(text: &str, budget: usize) -> &str {
    let mut end = floor_boundary(text, budget);
    if let Some(newline) = text[..end].rfind('\n')
        && newline + 1 >= budget / 2
    {
        end = newline + 1;
    }
    &text[..end]
}

/// The tail, by the same rule but read from the end.
fn tail_slice(text: &str, budget: usize) -> &str {
    let start = ceil_boundary(text, text.len().saturating_sub(budget));
    let rest = &text[start..];
    if let Some(newline) = rest.find('\n')
        && newline <= budget / 2
    {
        return &rest[newline + 1..];
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(tokens: usize) -> Overflow {
        Overflow::new(&Context::root()).with_budget(tokens)
    }

    #[test]
    fn vua_ngan_sach_thi_khong_cat() {
        assert!(bench(100).split("ngắn").is_none());
    }

    /// Proof that counting lines is wrong: three lines, each very long.
    #[test]
    fn it_dong_ma_dong_rat_dai_van_bi_cat() {
        let full = format!(
            "{}\n{}\n{}\n",
            "a".repeat(4000),
            "b".repeat(4000),
            "c".repeat(4000)
        );
        let split = bench(100).split(&full).expect("vượt 400 byte thì phải cắt");
        assert!(split.head.starts_with('a'));
        assert!(split.tail.ends_with('c') || split.tail.ends_with("c\n"));
        assert!(split.head.len() + split.tail.len() < full.len());
    }

    /// A single line, with no `\n` to cut neatly at.
    #[test]
    fn mot_dong_duy_nhat_van_cat_duoc_giua_dong() {
        let full = "x".repeat(10_000);
        let split = bench(100).split(&full).expect("phải cắt");
        assert!(!split.head.is_empty(), "cắt giữa dòng còn hơn trả về rỗng");
        assert!(!split.tail.is_empty());
    }

    #[test]
    fn cat_khong_bao_gio_roi_vao_giua_mot_ky_tu() {
        let full = "đường dẫn tiếng Việt ".repeat(500);
        let split = bench(50).split(&full).expect("phải cắt");
        // A `&str` only exists on character boundaries, so a mistake would already have panicked above.
        assert!(full.starts_with(split.head));
        assert!(full.ends_with(split.tail));
    }
}
