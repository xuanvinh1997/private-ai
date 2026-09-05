//! `git.log` — commit history.
//!
//! Asked for with an explicit `--pretty` format built out of two control characters rather
//! than read back from git's human layout. `%x1e` starts a record and `%x1f` ends a field,
//! neither of which occurs in real prose, so a commit message containing blank lines, `---`
//! or a diff of its own cannot make the parser lose its place.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::render::{OVERFLOW_NOTICE, finish};
use crate::repo::{Repo, check_rev, check_text};
use crate::tools::read_meta;

/// Records per call by default. Twenty commits is a reading of recent history; a hundred is
/// a research project that wants `paths` or `grep` instead.
const DEFAULT_MAX_COUNT: usize = 20;
/// Hard ceiling on `max_count`.
const MAX_MAX_COUNT: usize = 200;
/// Line budget for the rendered text, on top of `max_count`: 200 commits with long bodies is
/// still far too much text even though it is a legal record count. Spent in whole commits —
/// see [`render_within`].
const DEFAULT_MAX_LINES: usize = 600;
const MAX_MAX_LINES: usize = 10_000;
/// Characters of a commit body kept per record. The subject carries the intent; the body is
/// context, and twenty commits' worth of full bodies is a context window on its own.
const MAX_BODY_CHARS: usize = 600;

/// Record separator; `%x1e` in the format string.
const RECORD: char = '\u{1e}';
/// Field separator; `%x1f` in the format string.
const FIELD: char = '\u{1f}';

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogArgs {
    /// Bắt đầu từ phiên bản nào (nhánh, tag, sha, hoặc khoảng `a..b`). Để trống là từ HEAD.
    pub rev: Option<String>,
    /// Chỉ lấy commit có đụng tới những đường dẫn này (tương đối so với gốc kho).
    pub paths: Option<Vec<String>>,
    /// Lọc theo tác giả, khớp một phần là đủ.
    pub author: Option<String>,
    /// Lọc theo nội dung thông điệp commit, khớp một phần là đủ.
    pub grep: Option<String>,
    /// Chỉ lấy commit từ mốc này trở đi, vd `2024-01-01` hay `2 weeks ago`.
    pub since: Option<String>,
    /// `true` để kèm danh sách tệp mỗi commit đã đụng tới.
    pub files: Option<bool>,
    /// Lấy tối đa bao nhiêu commit. Mặc định 20, trần 200.
    pub max_count: Option<usize>,
    /// Tối đa bao nhiêu dòng kết quả. Mặc định 600, trần 10000.
    pub max_lines: Option<usize>,
}

/// One commit, after parsing.
struct Commit {
    hash: String,
    short: String,
    author: String,
    email: String,
    date: String,
    subject: String,
    body: String,
    /// `--name-status` rows, when asked for: `M\tsrc/a.rs`.
    files: Vec<String>,
}

pub struct GitLog {
    repo: Arc<Repo>,
    overflow: Overflow,
}

impl GitLog {
    pub const NAME: &'static str = "git.log";

    pub fn new(repo: Arc<Repo>, overflow: Overflow) -> GitLog {
        GitLog { repo, overflow }
    }
}

#[async_trait]
impl Tool for GitLog {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GitLog::NAME,
            "Đọc lịch sử commit của kho git thuộc dự án đang mở. Mặc định lấy 20 commit gần \
             nhất tính từ HEAD. Thu hẹp bằng `paths` để biết một tệp đã đổi vì lý do gì, hoặc \
             bằng `grep`/`author`/`since`. Trả về sha đầy đủ — dùng nó với `git.show` để đọc \
             nội dung một commit.",
            json_schema_for::<LogArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        read_meta()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: LogArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let max_count = args
            .max_count
            .unwrap_or(DEFAULT_MAX_COUNT)
            .clamp(1, MAX_MAX_COUNT);
        let max_lines = args
            .max_lines
            .unwrap_or(DEFAULT_MAX_LINES)
            .clamp(1, MAX_MAX_LINES);

        let mut argv = vec![
            "log".to_string(),
            "--no-color".to_string(),
            format!("--max-count={max_count}"),
            // ISO with an offset, so a date is unambiguous without knowing the machine's zone.
            "--date=iso-strict".to_string(),
            // Sáu trường cố định, rồi `%b`, rồi một `%x1f` chốt đuôi để phần
            // `--name-status` (nếu có) rơi vào một trường riêng thay vì dính vào body.
            "--pretty=format:%x1e%H%x1f%h%x1f%an%x1f%ae%x1f%ad%x1f%s%x1f%b%x1f".to_string(),
        ];
        if args.files.unwrap_or(false) {
            argv.push("--name-status".to_string());
            argv.push("--no-ext-diff".to_string());
        }
        // Filters are glued to their option with `=`, so their value can never be read as a
        // separate word however it starts.
        if let Some(author) = &args.author {
            argv.push(format!("--author={}", check_text(author, "author")?));
        }
        if let Some(grep) = &args.grep {
            argv.push(format!("--grep={}", check_text(grep, "grep")?));
        }
        if let Some(since) = &args.since {
            argv.push(format!("--since={}", check_text(since, "since")?));
        }
        if let Some(rev) = &args.rev {
            argv.push(check_rev(rev)?);
        }
        let paths = match &args.paths {
            Some(paths) => self.repo.relatives(paths)?,
            None => Vec::new(),
        };
        argv.push("--".to_string());
        argv.extend(paths.iter().cloned());

        let out = self.repo.run(&argv, &call.cancel_token()).await?;
        let commits = parse(&out.stdout);
        if commits.is_empty() {
            return Ok(ToolOutcome::ok(
                "Không có commit nào khớp yêu cầu. Kho có thể chưa có commit, hoặc các bộ lọc \
                 quá hẹp.",
            )
            .with_structured(json!({ "shape": "git.log", "commits": [] })));
        }

        let (mut text, shown) = render_within(&commits, max_lines);
        if shown < commits.len() {
            text.push_str(&format!(
                "\n\n[… còn {} commit nữa không hiện (tổng {}) cho vừa giới hạn {max_lines} \
                 dòng. Gọi lại với `max_count` nhỏ hơn, `paths` hẹp hơn, hoặc `max_lines` lớn hơn.]",
                commits.len() - shown,
                commits.len(),
            ));
        }
        if out.overflowed {
            text.push('\n');
            text.push_str(OVERFLOW_NOTICE);
        }

        let structured = json!({
            "shape": "git.log",
            "shown": shown,
            "total": commits.len(),
            "truncated": shown < commits.len(),
            // Only the commits the text shows. `structured` is not free: `pai-mcp`'s expose
            // layer forwards it as `structured_content`, so a commit dropped from the render
            // but kept here would carry its body past the very budget we just applied.
            "commits": commits.iter().take(shown).map(|commit| json!({
                "hash": commit.hash,
                "short": commit.short,
                "author": commit.author,
                "email": commit.email,
                "date": commit.date,
                "subject": commit.subject,
                "body": commit.body,
                "files": commit.files,
            })).collect::<Vec<_>>(),
        });
        Ok(finish(
            &self.overflow,
            call,
            text,
            structured,
            "Gọi lại `git.log` với `max_count` nhỏ hơn để đọc trọn vẹn.",
        ))
    }
}

/// Split the record stream into commits.
fn parse(stdout: &str) -> Vec<Commit> {
    stdout
        .split(RECORD)
        // The text before the first `%x1e` is empty; with `--name-status` it is nothing at all.
        .filter(|record| !record.trim().is_empty())
        .filter_map(parse_one)
        .collect()
}

fn parse_one(record: &str) -> Option<Commit> {
    let fields: Vec<&str> = record.split(FIELD).collect();
    // Six fixed fields, then the body, then the trailing block after the format's last `%x1f`.
    if fields.len() < 8 {
        return None;
    }
    let tail = fields.len() - 1;
    // The body is everything between field 6 and the trailing block. Joining rather than
    // indexing keeps a body that happens to contain a `%x1f` byte intact instead of shifting
    // every later field by one.
    let body = fields[6..tail].join(&FIELD.to_string());
    let files = fields[tail]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    Some(Commit {
        hash: fields[0].trim().to_string(),
        short: fields[1].trim().to_string(),
        author: fields[2].trim().to_string(),
        email: fields[3].trim().to_string(),
        date: fields[4].trim().to_string(),
        subject: fields[5].trim().to_string(),
        body: truncate_body(body.trim()),
        files,
    })
}

/// Keep the head of a body, on a character boundary, and say when there was more.
fn truncate_body(body: &str) -> String {
    if body.chars().count() <= MAX_BODY_CHARS {
        return body.to_string();
    }
    let head: String = body.chars().take(MAX_BODY_CHARS).collect();
    format!("{head}\n[… phần còn lại của thông điệp commit đã bị cắt; đọc trọn bằng `git.show`]")
}

/// Render commits into the line budget, and say how many of them fitted.
///
/// Whole records or nothing. Cutting the joined text at an arbitrary line — which is what a
/// generic line cap does — leaves a half-written commit whose `sha:` footer belongs to the
/// record above it, and that is the one line a model is most likely to copy into `git.show`.
/// The first commit always goes in, however long it is: an empty answer helps nobody.
fn render_within(commits: &[Commit], max_lines: usize) -> (String, usize) {
    let mut text = String::new();
    let mut used = 0usize;
    let mut shown = 0usize;
    for commit in commits {
        let block = render_one(commit);
        let lines = block.lines().count();
        if shown > 0 && used + lines > max_lines {
            break;
        }
        if shown > 0 {
            text.push_str("\n\n");
        }
        text.push_str(&block);
        used += lines;
        shown += 1;
    }
    (text, shown)
}

fn render_one(commit: &Commit) -> String {
    let mut text = format!(
        "{} — {} <{}> — {}\n  {}",
        commit.short, commit.author, commit.email, commit.date, commit.subject
    );
    if !commit.body.is_empty() {
        for line in commit.body.lines() {
            text.push_str("\n  ");
            text.push_str(line);
        }
    }
    for file in &commit.files {
        text.push_str("\n  · ");
        text.push_str(file);
    }
    // The full sha last, so it is easy to copy into `git.show` and does not crowd the subject.
    text.push_str(&format!("\n  sha: {}", commit.hash));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str, subject: &str, body: &str, tail: &str) -> String {
        format!(
            "{RECORD}{hash}{FIELD}{short}{FIELD}Ai Đó{FIELD}ai@vidu.vn{FIELD}\
             2024-01-02T03:04:05+07:00{FIELD}{subject}{FIELD}{body}{FIELD}{tail}",
            short = &hash[..7.min(hash.len())]
        )
    }

    #[test]
    fn parse_reads_every_field() {
        let stdout = record("a".repeat(40).as_str(), "Sửa lỗi", "Chi tiết\ndòng hai", "");
        let commits = parse(&stdout);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "Sửa lỗi");
        assert_eq!(commits[0].body, "Chi tiết\ndòng hai");
        assert_eq!(commits[0].author, "Ai Đó");
        assert!(commits[0].files.is_empty());
    }

    #[test]
    fn parse_reads_name_status_after_the_body() {
        let stdout = record("b".repeat(40).as_str(), "Thêm", "", "\nM\tsrc/a.rs\nA\tsrc/b.rs\n");
        let commits = parse(&stdout);
        assert_eq!(commits[0].files, ["M\tsrc/a.rs", "A\tsrc/b.rs"]);
    }

    #[test]
    fn parse_survives_a_body_full_of_traps() {
        // A commit message quoting its own diff used to be enough to desynchronise a parser
        // that split on blank lines.
        let body = "diff --git a/x b/x\n\n--\n\u{1f}còn sót một dấu phân cách";
        let stdout = record("c".repeat(40).as_str(), "Chủ đề", body, "");
        let commits = parse(&stdout);
        assert_eq!(commits.len(), 1);
        assert!(commits[0].body.contains("còn sót một dấu phân cách"));
        assert_eq!(commits[0].subject, "Chủ đề");
    }

    #[test]
    fn parse_reads_several_records() {
        let stdout = format!(
            "{}{}",
            record("d".repeat(40).as_str(), "Một", "", ""),
            record("e".repeat(40).as_str(), "Hai", "", "")
        );
        let commits = parse(&stdout);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[1].subject, "Hai");
    }

    #[test]
    fn render_within_stops_on_a_record_boundary() {
        let stdout = format!(
            "{}{}{}",
            record("d".repeat(40).as_str(), "Một", "", ""),
            record("e".repeat(40).as_str(), "Hai", "", ""),
            record("f".repeat(40).as_str(), "Ba", "", "")
        );
        let commits = parse(&stdout);
        // Each record renders as three lines (tiêu đề, chủ đề, sha), so a five-line budget
        // holds one whole commit and must not start a second.
        let (text, shown) = render_within(&commits, 5);
        assert_eq!(shown, 1);
        assert!(text.contains("Một"), "{text}");
        assert!(!text.contains("Hai"), "{text}");

        let (text, shown) = render_within(&commits, 600);
        assert_eq!(shown, 3);
        assert!(text.contains("Ba"), "{text}");
    }

    #[test]
    fn render_within_always_shows_the_first_commit() {
        let commits = parse(&record("a".repeat(40).as_str(), "Chủ đề", "thân", ""));
        // Budget of one line, record of four: an answer that says nothing is worse than an
        // answer over budget, and the token budget behind it still folds what is too long.
        let (text, shown) = render_within(&commits, 1);
        assert_eq!(shown, 1);
        assert!(text.contains("Chủ đề"), "{text}");
    }

    #[test]
    fn truncate_body_announces_the_cut() {
        let long = "x".repeat(MAX_BODY_CHARS + 10);
        let cut = truncate_body(&long);
        assert!(cut.contains("đã bị cắt"));
        assert!(truncate_body("ngắn").ends_with("ngắn"));
    }
}
