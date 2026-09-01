//! Dựng hunk diff kèm **số dòng thật trong tệp**.
//!
//! Giao diện vẽ số dòng cạnh mỗi dòng diff. Nếu hunk không mang theo vị trí của nó trong
//! tệp thì giao diện đánh số từ 1, và người đọc thấy một con số trông đúng nhưng chỉ vào
//! chỗ khác — sai lệch im lặng, kiểu tệ nhất. Nên `old_start`/`new_start` được tính ở
//! đây, chỗ duy nhất còn biết cả hai bản văn bản.

use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};

/// Bao nhiêu dòng không đổi giữ lại quanh mỗi thay đổi.
const CONTEXT: usize = 3;

/// Một tệp mới: không có bản cũ để so.
pub fn created(path: &str, content: &str) -> Value {
    json!([{
        "path": path,
        "old_text": Value::Null,
        "new_text": content,
        "old_start": Value::Null,
        "new_start": 1,
    }])
}

/// Hunk giữa hai bản văn bản. Trả mảng rỗng nếu không có gì đổi.
pub fn between(path: &str, old: &str, new: &str) -> Value {
    let diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();

    for group in diff.grouped_ops(CONTEXT) {
        let (mut old_text, mut new_text) = (String::new(), String::new());
        // `grouped_ops` cho các chỉ số 0-based; số dòng cho người đọc đếm từ 1.
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
