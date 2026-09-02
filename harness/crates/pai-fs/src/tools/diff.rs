//! Build diff hunks carrying **real line numbers from the file**.
//!
//! The UI draws a line number beside every diff line. If a hunk does not carry its position
//! in the file, the UI numbers from 1 and the reader sees a number that looks right and
//! points somewhere else — a silent discrepancy, the worst kind. So `old_start`/`new_start`
//! are computed here, the only place that still has both versions of the text.

use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};

/// How many unchanged lines to keep around each change.
const CONTEXT: usize = 3;

/// A new file: there is no old version to compare against.
pub fn created(path: &str, content: &str) -> Value {
    json!([{
        "path": path,
        "old_text": Value::Null,
        "new_text": content,
        "old_start": Value::Null,
        "new_start": 1,
    }])
}

/// The hunks between two texts. Returns an empty array when nothing changed.
pub fn between(path: &str, old: &str, new: &str) -> Value {
    let diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();

    for group in diff.grouped_ops(CONTEXT) {
        let (mut old_text, mut new_text) = (String::new(), String::new());
        // `grouped_ops` gives 0-based indices; line numbers for readers count from 1.
        let old_start = group.first().map(|op| op.old_range().start + 1);
        let new_start = group.first().map(|op| op.new_range().start + 1);

        for op in &group {
            for change in diff.iter_changes(op) {
                match change.tag() {
                    ChangeTag::Delete => old_text.push_str(change.value()),
                    ChangeTag::Insert => new_text.push_str(change.value()),
                    ChangeTag::Equal => {
                        old_text.push_str(change.value());
                        new_text.push_str(change.value());
                    }
                }
            }
        }

        hunks.push(json!({
            "path": path,
            "old_text": old_text,
            "new_text": new_text,
            "old_start": old_start,
            "new_start": new_start,
        }));
    }

    Value::Array(hunks)
}
