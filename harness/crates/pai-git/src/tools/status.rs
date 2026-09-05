//! `git.status` — what is different between the working tree, the index and HEAD.
//!
//! Parsed from `--porcelain`, which is the format git promises not to change, and rendered
//! into three groups because that is the distinction the model keeps getting wrong: a file
//! that is staged and a file that is merely edited are not the same file to a commit.

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::render::{OVERFLOW_NOTICE, finish};
use crate::repo::Repo;
use crate::tools::read_meta;

/// How many changed files we print by default. A working tree with more than this either
/// needs `paths` or does not need reading file by file at all.
const DEFAULT_MAX_ENTRIES: usize = 200;
/// Hard ceiling on `max_entries`.
const MAX_MAX_ENTRIES: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatusArgs {
    /// Chỉ xét những đường dẫn này (tương đối so với gốc kho). Để trống là xét cả kho.
    pub paths: Option<Vec<String>>,
    /// Tối đa bao nhiêu mục được liệt kê. Mặc định 200, trần 2000. (Một tệp vừa nằm trong chỉ
    /// mục vừa còn sửa tiếp ngoài cây làm việc chiếm hai mục, vì đó là hai thông tin khác nhau.)
    pub max_entries: Option<usize>,
}

/// One changed file, already split into the two halves git reports separately.
struct Entry {
    /// Mã hai ký tự của git, giữ nguyên để ai quen đọc `git status --porcelain` vẫn nhận ra.
    code: String,
    path: String,
    /// Tên cũ, khi git báo một lần đổi tên hoặc sao chép.
    orig: Option<String>,
    group: Group,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Staged,
    Unstaged,
    Untracked,
    Conflict,
}

impl Group {
    fn heading(self) -> &'static str {
        match self {
            Group::Conflict => "Đang xung đột",
            Group::Staged => "Đã đưa vào chỉ mục, sẵn sàng commit",
            Group::Unstaged => "Đã sửa nhưng chưa đưa vào chỉ mục",
            Group::Untracked => "Chưa được git theo dõi",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Group::Conflict => "conflict",
            Group::Staged => "staged",
            Group::Unstaged => "unstaged",
            Group::Untracked => "untracked",
        }
    }
}

pub struct GitStatus {
    repo: Arc<Repo>,
    overflow: Overflow,
}

impl GitStatus {
    pub const NAME: &'static str = "git.status";

    pub fn new(repo: Arc<Repo>, overflow: Overflow) -> GitStatus {
        GitStatus { repo, overflow }
    }
}

#[async_trait]
impl Tool for GitStatus {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GitStatus::NAME,
            "Xem kho git của dự án đang mở có gì thay đổi: nhánh hiện tại, tệp đã đưa vào \
             chỉ mục, tệp đã sửa mà chưa đưa vào, tệp chưa được theo dõi, và tệp đang xung \
             đột. Chỉ đọc, không thay đổi gì. Dùng nó trước khi kết luận về trạng thái cây \
             làm việc, đừng đoán từ những tệp bạn vừa sửa.",
            json_schema_for::<StatusArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        read_meta()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: StatusArgs = serde_json::from_value(Value::Object(call.arguments.clone()))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let max_entries = args
            .max_entries
            .unwrap_or(DEFAULT_MAX_ENTRIES)
            .clamp(1, MAX_MAX_ENTRIES);

        let mut argv = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "--branch".to_string(),
        ];
        if let Some(paths) = &args.paths {
            let paths = self.repo.relatives(paths)?;
            // `--` first: after it git reads every remaining word as a path, never an option.
            argv.push("--".to_string());
            argv.extend(paths);
        }

        let out = self.repo.run(&argv, &call.cancel_token()).await?;
        let (branch, entries) = parse(&out.stdout);

        let total = entries.len();
        let shown = &entries[..total.min(max_entries)];
        let omitted = total - shown.len();

        let mut text = match &branch {
            Some(branch) => format!("Nhánh: {branch}"),
            // A repository with no commits yet has no branch line to report; say so rather
            // than print nothing, or the empty result reads like a failure.
            None => "Nhánh: (không xác định được — kho có thể chưa có commit nào)".to_string(),
        };
        if total == 0 {
            text.push_str("\n\nCây làm việc sạch: không có thay đổi nào.");
        }
        for group in [
            Group::Conflict,
            Group::Staged,
            Group::Unstaged,
            Group::Untracked,
        ] {
            let rows: Vec<&Entry> = shown.iter().filter(|item| item.group == group).collect();
            if rows.is_empty() {
                continue;
            }
            text.push_str(&format!("\n\n{} ({}):", group.heading(), rows.len()));
            for row in rows {
                text.push_str(&format!("\n  {:<9} {}", gloss(&row.code, row.group), row.path));
                // The old name earns its place on the line: without it a rename reads as a
                // brand-new file, and the model goes looking for the deletion that matches it.
                if let Some(orig) = &row.orig {
                    text.push_str(&format!(" (đổi tên từ {orig})"));
                }
            }
        }
        if omitted > 0 {
            text.push_str(&format!(
                "\n\n[… còn {omitted} mục nữa không liệt kê (tổng {total}). Gọi lại với \
                 `max_entries` lớn hơn, hoặc thu hẹp bằng `paths`.]"
            ));
        }
        if out.overflowed {
            text.push_str("\n\n");
            text.push_str(OVERFLOW_NOTICE);
        }

        let structured = json!({
            "shape": "git.status",
            "branch": branch,
            "total": total,
            "truncated": omitted > 0,
            "entries": shown.iter().map(|row| json!({
                "code": row.code,
                "path": row.path,
                "orig": row.orig,
                "group": row.group.key(),
            })).collect::<Vec<_>>(),
        });
        Ok(finish(
            &self.overflow,
            call,
            text,
            structured,
            "Gọi lại `git.status` với `paths` để chỉ xem một phần kho.",
        ))
    }
}

/// Split `--porcelain --branch` output into the branch line and the file entries.
fn parse(stdout: &str) -> (Option<String>, Vec<Entry>) {
    let mut branch = None;
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            branch = Some(rest.trim().to_string());
            continue;
        }
        // `XY path`: two status columns, a space, then the path. Anything shorter is not an
        // entry — a truncated last line, for instance.
        if line.len() < 4 {
            continue;
        }
        // `get`, not `line[..2]`: git's bytes reach us through `from_utf8_lossy`, and a single
        // replacement character at the front of a mangled line would make an index slice a
        // panic in the middle of a tool call. A line that does not split there is not an entry.
        let (Some(code), Some(rest)) = (line.get(..2), line.get(3..)) else {
            continue;
        };
        let code = code.to_string();
        // `R  cũ -> mới` and `C  cũ -> mới`: porcelain v1 puts both names on one line. Leaving
        // the arrow inside `path` would hand the model a string that is not a path and that it
        // would then pass back to `git.log` or `read`.
        let (path, orig) = match rest.split_once(" -> ") {
            Some((from, to)) => (to.to_string(), Some(from.to_string())),
            None => (rest.to_string(), None),
        };
        let mut chars = code.chars();
        let (x, y) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));

        // Conflicts first: `UU`, `AA` and `DD` also have non-space columns, so testing them
        // after "staged" would file a conflict as a normal staged change.
        if x == 'U' || y == 'U' || code == "AA" || code == "DD" {
            entries.push(Entry { code, path, orig, group: Group::Conflict });
            continue;
        }
        if code == "??" {
            entries.push(Entry { code, path, orig, group: Group::Untracked });
            continue;
        }
        if code == "!!" {
            // Ignored files; only ever present with `--ignored`, which we do not pass.
            continue;
        }
        // A file can be in both halves at once — staged edit plus a further edit on disk —
        // and it is listed in both, because those are two different pieces of information.
        if x != ' ' {
            entries.push(Entry {
                code: code.clone(),
                path: path.clone(),
                orig: orig.clone(),
                group: Group::Staged,
            });
        }
        if y != ' ' {
            entries.push(Entry { code, path, orig, group: Group::Unstaged });
        }
    }
    (branch, entries)
}

/// The Vietnamese word for a status code, from whichever column the group belongs to.
fn gloss(code: &str, group: Group) -> &'static str {
    let mut chars = code.chars();
    let (x, y) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));
    let letter = match group {
        Group::Untracked => return "mới",
        Group::Conflict => return "xung đột",
        Group::Staged => x,
        Group::Unstaged => y,
    };
    match letter {
        'M' => "sửa",
        'A' => "thêm",
        'D' => "xoá",
        'R' => "đổi tên",
        'C' => "sao chép",
        'T' => "đổi kiểu",
        _ => "khác",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_splits_branch_and_groups() {
        let stdout = "## main...origin/main [ahead 1]\n\
                      M  staged.rs\n\
                      \x20M worktree.rs\n\
                      MM ca_hai.rs\n\
                      ?? moi.txt\n\
                      UU xung_dot.rs\n";
        let (branch, entries) = parse(stdout);
        assert_eq!(branch.as_deref(), Some("main...origin/main [ahead 1]"));

        let of = |group: Group| {
            entries
                .iter()
                .filter(|item| item.group == group)
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(of(Group::Staged), ["staged.rs", "ca_hai.rs"]);
        assert_eq!(of(Group::Unstaged), ["worktree.rs", "ca_hai.rs"]);
        assert_eq!(of(Group::Untracked), ["moi.txt"]);
        assert_eq!(of(Group::Conflict), ["xung_dot.rs"]);
    }

    #[test]
    fn parse_splits_a_rename_into_both_names() {
        let (_, entries) = parse("## main\nR  cu.rs -> moi.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "moi.rs");
        assert_eq!(entries[0].orig.as_deref(), Some("cu.rs"));
    }

    #[test]
    fn parse_does_not_panic_on_a_mangled_line() {
        // What `from_utf8_lossy` leaves behind when git's bytes are not UTF-8: a three-byte
        // replacement character where the two status columns should be. Slicing by index here
        // was a panic; the line is simply not an entry.
        let (branch, entries) = parse("## main\n\u{fffd}x moi.rs\n M ok.rs\n");
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "ok.rs");
    }

    #[test]
    fn gloss_reads_the_column_of_its_own_group() {
        assert_eq!(gloss("MD", Group::Staged), "sửa");
        assert_eq!(gloss("MD", Group::Unstaged), "xoá");
        assert_eq!(gloss("??", Group::Untracked), "mới");
    }
}
