//! Turning git's output into something with a ceiling on it.
//!
//! Two ceilings apply, in this order, and they answer different questions:
//!
//! * [`cap_lines`] is the *semantic* one. It says "you asked for more than a tool result
//!   should carry"; the caller controls it with `max_lines` and can raise it.
//! * [`finish`] then applies [`Overflow`], the *context window* one. It is not negotiable,
//!   it folds head and tail rather than cutting the end off, and it puts the whole text in
//!   the spill store so nothing is actually lost.
//!
//! Both announce themselves. A silent cut is a lie to the model: it will conclude that a
//! function has one caller because the fifty-first line of `git log` never arrived.

use pai_tools::{Invocation, Overflow, ToolOutcome};
use serde_json::Value;

/// A text cut to a line budget.
pub struct Capped {
    /// The kept lines, with no notice attached — [`Capped::render`] adds that.
    pub text: String,
    pub kept: usize,
    pub omitted: usize,
    pub total: usize,
}

impl Capped {
    pub fn truncated(&self) -> bool {
        self.omitted > 0
    }

    /// The kept text plus, when something was dropped, one line saying exactly what and how
    /// to get the rest. `hint` is the tool's own instruction for asking again.
    pub fn render(&self, hint: &str) -> String {
        if !self.truncated() {
            return self.text.clone();
        }
        format!(
            "{}\n[… đã cắt {} dòng cuối trên tổng {} dòng cho vừa giới hạn {} dòng. {hint}]",
            self.text.trim_end_matches('\n'),
            self.omitted,
            self.total,
            self.kept,
        )
    }
}

/// Keep the first `max` lines of `text`.
///
/// The head, not the tail: git orders every one of these commands most-relevant-first —
/// newest commit, first hunk, first line blamed — so the front is the part worth keeping.
pub fn cap_lines(text: &str, max: usize) -> Capped {
    let max = max.max(1);
    let total = text.lines().count();
    if total <= max {
        return Capped {
            text: text.to_string(),
            kept: total,
            omitted: 0,
            total,
        };
    }
    let kept: Vec<&str> = text.lines().take(max).collect();
    Capped {
        text: kept.join("\n"),
        kept: max,
        omitted: total - max,
        total,
    }
}

/// Said when git itself produced more than [`crate::repo::MAX_STDOUT_BYTES`], which is a
/// different and worse fact than being over the line budget: the tail was never read at all,
/// so not even the spill store has it.
pub const OVERFLOW_NOTICE: &str = "[Cảnh báo: `git` in ra nhiều hơn mức tối đa tool này đọc \
được, nên phần đuôi đã bị mất hẳn. Hãy thu hẹp yêu cầu — ít commit hơn, hoặc giới hạn vào \
một vài đường dẫn.]";

/// Apply the token budget and build the outcome. Every tool in this crate ends here, so the
/// spill handle and the resume hint are attached the same way in all five.
pub fn finish(
    overflow: &Overflow,
    call: &Invocation,
    text: String,
    structured: Value,
    hint: &str,
) -> ToolOutcome {
    let folded = overflow.fold(&call.name, text, |_| hint.to_string());
    let mut outcome = ToolOutcome::ok(folded.content).with_structured(structured);
    if let Some(handle) = folded.spill {
        outcome.meta.insert("spill".into(), handle.to_json());
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_lines_keeps_everything_when_it_fits() {
        let capped = cap_lines("a\nb\nc", 10);
        assert!(!capped.truncated());
        assert_eq!(capped.render("thử lại"), "a\nb\nc");
    }

    #[test]
    fn cap_lines_says_how_much_it_dropped() {
        let text = (1..=10).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
        let capped = cap_lines(&text, 3);
        assert_eq!(capped.kept, 3);
        assert_eq!(capped.omitted, 7);
        assert_eq!(capped.total, 10);
        let rendered = capped.render("gọi lại với `max_lines` lớn hơn.");
        assert!(rendered.starts_with("1\n2\n3\n[…"), "{rendered}");
        assert!(rendered.contains("đã cắt 7 dòng cuối trên tổng 10 dòng"), "{rendered}");
    }

    #[test]
    fn cap_lines_never_takes_a_zero_budget() {
        let capped = cap_lines("a\nb", 0);
        assert_eq!(capped.kept, 1);
    }
}
