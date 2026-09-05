//! `git.blame` — which commit last touched each line of a file.
//!
//! Windowed by construction. Blame is the one command here whose output is exactly as long
//! as its input, so `-L start,+limit` is passed to git rather than trimming afterwards: a
//! window git never computed is a window we never have to pay for.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::GitError;
use crate::render::{OVERFLOW_NOTICE, finish};
use crate::repo::{Repo, check_rev};
use crate::tools::read_meta;

/// Lines blamed per call by default. Enough to cover a function and its neighbours; a whole
/// file's authorship at once is almost never the question being asked.
const DEFAULT_LIMIT: usize = 200;
/// Hard ceiling on `limit`.
const MAX_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BlameArgs {
    /// Tệp cần truy nguồn, đường dẫn tương đối so với gốc kho.
    pub file: String,
    /// Bắt đầu từ dòng nào, đếm từ 1. Mặc định 1.
    pub start: Option<usize>,
    /// Truy tối đa bao nhiêu dòng kể từ `start`. Mặc định 200, trần 2000.
    pub limit: Option<usize>,
    /// Truy nguồn tại phiên bản nào thay vì cây làm việc hiện tại.
    pub rev: Option<String>,
}

/// One blamed line, parsed out of git's default layout.
struct Line {
    hash: String,
    author: String,
    date: String,
    number: usize,
    text: String,
}

pub struct GitBlame {
    repo: Arc<Repo>,
    overflow: Overflow,
}

impl GitBlame {
    pub const NAME: &'static str = "git.blame";

    pub fn new(repo: Arc<Repo>, overflow: Overflow) -> GitBlame {
        GitBlame { repo, overflow }
    }
}

#[async_trait]
impl Tool for GitBlame {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GitBlame::NAME,
            "Xem mỗi dòng của một tệp lần cuối được commit nào sửa, do ai và khi nào. Mặc \
             định truy 200 dòng kể từ đầu tệp; dùng `start` và `limit` để nhắm đúng đoạn cần \
             hỏi. Dùng nó khi cần biết một dòng code có từ đâu, rồi đưa sha sang `git.show` \
             để đọc lý do.",
            json_schema_for::<BlameArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        read_meta()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: BlameArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let file = self.repo.relative(&args.file)?;
        // Line 0 does not exist; git would refuse it, and clamping says so more usefully.
        let start = args.start.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

        let mut argv = vec![
            "blame".to_string(),
            // Numeric and short: the full ISO timestamp costs a third of every line for
            // information nobody blames a line to find out.
            "--date=short".to_string(),
            // Blame runs `diff.<driver>.textconv` too, and there it does not merely execute a
            // configured program — it replaces the content column, so every line comes back
            // attributed to the converter's output rather than to the file. See `git.diff`.
            "--no-textconv".to_string(),
            // `+limit` counts from `start`, so the *end* of the window may run past the end of
            // the file and git simply stops. The start may not: `-L9` in a five-line file is a
            // fatal error, handled below rather than pretended away.
            format!("-L{start},+{limit}"),
        ];
        if let Some(rev) = &args.rev {
            argv.push(check_rev(rev)?);
        }
        argv.push("--".to_string());
        argv.push(file.clone());

        let out = match self.repo.run(&argv, &call.cancel_token()).await {
            Ok(out) => out,
            // A `start` past the end of the file is an argument the model can fix, so it must
            // come back as `Invalid`. As `Failed` it reads like a broken machine, and the
            // usual reaction to that is to retry the identical call.
            Err(GitError::Command { detail, .. }) if detail.contains("has only") => {
                return Err(ToolError::Invalid(format!(
                    "`{file}` ngắn hơn `start` = {start} nên không có gì để truy nguồn (git nói{detail}). \
                     Gọi lại với `start` nhỏ hơn."
                )));
            }
            Err(err) => return Err(err.into()),
        };
        // Count what git printed, not what parsed: a line the parser could not read is still a
        // line the model is being shown, and using the parsed count here would understate the
        // range in the header and mis-detect a full window below.
        let produced = out.stdout.lines().count();
        let lines = parse(&out.stdout);
        if produced == 0 {
            return Ok(ToolOutcome::ok(format!(
                "`{file}` không có dòng nào trong khoảng đã hỏi (từ dòng {start}). Tệp có thể rỗng."
            ))
            .with_structured(json!({ "shape": "git.blame", "file": file, "lines": [] })));
        }

        let mut text = format!(
            "{file} — dòng {start}..{}\n{}",
            start + produced - 1,
            out.stdout.trim_end_matches('\n')
        );
        // Exactly `limit` lines back means the window filled up, which is the only signal we
        // have that the file continues; saying so costs one line and prevents a wrong
        // "that's the whole file" conclusion.
        if produced >= limit {
            text.push_str(&format!(
                "\n[Cửa sổ đã đầy {limit} dòng — tệp có thể còn tiếp. Gọi lại với `start: {}`.]",
                start + limit
            ));
        }
        if out.overflowed {
            text.push('\n');
            text.push_str(OVERFLOW_NOTICE);
        }

        let structured = json!({
            "shape": "git.blame",
            "file": file,
            "start": start,
            "lines": lines.iter().map(|line| json!({
                "number": line.number,
                "hash": line.hash,
                "author": line.author,
                "date": line.date,
                "text": line.text,
            })).collect::<Vec<_>>(),
        });
        Ok(finish(
            &self.overflow,
            call,
            text,
            structured,
            "Gọi lại `git.blame` với `limit` nhỏ hơn để đọc trọn vẹn.",
        ))
    }
}

/// `^a1b2c3d (Ai Đó 2024-01-02 12) nội dung` → one [`Line`].
///
/// A line that does not fit the shape is skipped rather than guessed at: the rendered text
/// the model reads is git's own output, so a parse failure costs the UI a row, not the answer.
fn parse(stdout: &str) -> Vec<Line> {
    stdout.lines().filter_map(parse_one).collect()
}

fn parse_one(raw: &str) -> Option<Line> {
    // The leading `^` marks a boundary commit — the sha is still the sha.
    let line = raw.strip_prefix('^').unwrap_or(raw);
    let open = line.find(" (")?;
    let close = line[open..].find(')')? + open;
    let hash = line[..open].split_whitespace().next()?.to_string();

    let inside: Vec<&str> = line[open + 2..close].split_whitespace().collect();
    // At least an author, a date and a line number.
    if inside.len() < 3 {
        return None;
    }
    let number: usize = inside[inside.len() - 1].parse().ok()?;
    let date = inside[inside.len() - 2].to_string();
    let author = inside[..inside.len() - 2].join(" ");
    // The content starts one space after `)`; a line whose content is empty ends right there.
    let text = line[close + 1..].strip_prefix(' ').unwrap_or("").to_string();

    Some(Line { hash, author, date, number, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_one_reads_a_normal_line() {
        let line = parse_one("a1b2c3d4 (Ai Đó 2024-01-02 12) fn main() {").expect("parse được");
        assert_eq!(line.hash, "a1b2c3d4");
        assert_eq!(line.author, "Ai Đó");
        assert_eq!(line.date, "2024-01-02");
        assert_eq!(line.number, 12);
        assert_eq!(line.text, "fn main() {");
    }

    #[test]
    fn parse_one_reads_a_boundary_commit() {
        let line = parse_one("^b2c3d4e (Người Khác 2020-05-06 1) đầu tệp").expect("parse được");
        assert_eq!(line.hash, "b2c3d4e");
        assert_eq!(line.number, 1);
    }

    #[test]
    fn parse_one_keeps_an_empty_content_line() {
        let line = parse_one("a1b2c3d4 (Ai Đó 2024-01-02 3)").expect("parse được");
        assert_eq!(line.text, "");
    }

    #[test]
    fn parse_skips_a_line_it_cannot_read() {
        assert!(parse_one("rác không theo khuôn").is_none());
        assert_eq!(parse("rác\na1b2c3d4 (Ai 2024-01-02 1) ok").len(), 1);
    }
}
