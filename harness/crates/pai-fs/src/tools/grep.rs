//! `grep` — search file contents.
//!
//! Uses `grep-searcher` + `grep-regex` + `ignore` directly — ripgrep's own internals as a
//! library. No process is spawned, so there is no external binary to package and maintain
//! across releases, and no dependency on whether the user's machine has `rg`. This is where
//! Rust wins most decisively over the Python version.

use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::path::FileRoots;

/// How many matches are collected into `meta` for the UI to draw.
const DISPLAY_CAP: usize = 250;

/// A hard cap on matches collected.
///
/// Without it, a pattern like `.` over a repo of a few hundred thousand files pulls tens of
/// millions of lines into memory before anyone can say anything. This cap bites at
/// *collection*, not at display: it stops the walk, rather than scanning everything and then
/// throwing it away.
const MATCH_CAP: usize = 5_000;

/// A time cap on the walk.
///
/// A large repo on a network drive can take longer to scan than the tool's own deadline, and
/// the model then gets a line saying "over 120 seconds" instead of the matches already
/// found. Partial results that say they are partial are useful; a silent timeout is not.
const SEARCH_DEADLINE: Duration = Duration::from_secs(20);

/// Why the walk stopped early.
#[derive(Clone, Copy)]
enum Stopped {
    MatchCap,
    Deadline,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepArgs {
    /// The regular expression, Rust regex syntax.
    pub pattern: String,
    /// The directory or file to search. Empty means the workspace root.
    pub path: Option<String>,
    /// Filter by file name, e.g. `*.rs`.
    pub include: Option<String>,
}

pub struct Grep {
    roots: FileRoots,
    overflow: Overflow,
}

impl Grep {
    pub const NAME: &'static str = "grep";

    pub fn new(roots: FileRoots, overflow: Overflow) -> Grep {
        Grep { roots, overflow }
    }
}

struct Hit {
    path: String,
    line: u64,
    text: String,
}

#[async_trait]
impl Tool for Grep {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Grep::NAME,
            "Tìm một biểu thức chính quy trong nội dung tệp. Bỏ qua tệp nhị phân và \
             những gì `.gitignore` loại trừ. Trên kho lớn việc tìm dừng sớm khi chạm trần \
             số khớp hoặc trần thời gian, và kết quả nói rõ khi điều đó xảy ra.",
            json_schema_for::<GrepArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: GrepArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let base =
            match &args.path {
                Some(path) => self
                    .roots
                    .resolve_read(Path::new(path))
                    .map_err(|err| ToolError::Invalid(err.to_string()))?,
                None => self.roots.roots().first().cloned().ok_or_else(|| {
                    ToolError::Invalid("chưa có thư mục nào được cấp quyền".into())
                })?,
            };

        let matcher = RegexMatcher::new_line_matcher(&args.pattern)
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let include = args.include.clone();
        let roots = self.roots.clone();

        let (hits, stopped) =
            tokio::task::spawn_blocking(move || -> Result<(Vec<Hit>, Option<Stopped>), String> {
                let mut walk = WalkBuilder::new(&base);
                if let Some(pattern) = &include {
                    let mut overrides = OverrideBuilder::new(&base);
                    overrides.add(pattern).map_err(|e| e.to_string())?;
                    walk.overrides(overrides.build().map_err(|e| e.to_string())?);
                }

                let mut searcher = SearcherBuilder::new()
                    // A NUL byte abandons the whole file: a binary file matching the
                    // regex emits thousands of junk lines and pushes every real result out
                    // of view.
                    .binary_detection(BinaryDetection::quit(0))
                    .line_number(true)
                    .build();

                let started = Instant::now();
                let mut hits: Vec<Hit> = Vec::new();
                let mut stopped = None;
                for entry in walk.build().flatten() {
                    // The match cap stops the walk, but the conclusion is **not** drawn
                    // here: it is drawn after the loop, because a single file can hit the
                    // cap on its own and then the loop ends without ever reaching this
                    // point.
                    if hits.len() >= MATCH_CAP {
                        break;
                    }
                    if started.elapsed() >= SEARCH_DEADLINE {
                        stopped = Some(Stopped::Deadline);
                        break;
                    }
                    if !entry.file_type().is_some_and(|t| t.is_file()) {
                        continue;
                    }
                    if roots.is_protected(entry.path()) {
                        continue;
                    }
                    let path = entry.path().display().to_string();
                    let _ = searcher.search_path(
                        &matcher,
                        entry.path(),
                        UTF8(|line, text| {
                            hits.push(Hit {
                                path: path.clone(),
                                line,
                                text: text.trim_end().to_string(),
                            });
                            // `false` stops this file immediately: one generated file can
                            // blow the cap by itself.
                            Ok(hits.len() < MATCH_CAP)
                        }),
                    );
                }
                // Hitting the cap is hitting the cap, whether the loop ended on the cap
                // or on running out of files: once there are `MATCH_CAP` matches there is
                // no way to know what else is out there.
                if stopped.is_none() && hits.len() >= MATCH_CAP {
                    stopped = Some(Stopped::MatchCap);
                }
                Ok((hits, stopped))
            })
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?
            .map_err(ToolError::Invalid)?;

        if hits.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có dòng nào khớp `{}`.",
                args.pattern
            )));
        }

        let rendered = hits
            .iter()
            .map(|hit| format!("{}:{}:{}", hit.path, hit.line, hit.text))
            .collect::<Vec<_>>()
            .join("\n");

        // Grouped by file for display: ten matches inside one file read more easily than
        // ten loose lines repeating the same path.
        let mut groups: Vec<serde_json::Value> = Vec::new();
        for hit in hits.iter().take(DISPLAY_CAP) {
            let entry = json!({ "line": hit.line, "text": hit.text });
            match groups.last_mut() {
                Some(group) if group["path"] == hit.path.as_str() => {
                    if let Some(list) = group["matches"].as_array_mut() {
                        list.push(entry);
                    }
                }
                _ => groups.push(json!({ "path": hit.path, "matches": [entry] })),
            }
        }

        let folded = self.overflow.fold(&call.name, rendered, |_| {
            "Thu hẹp bằng `path` hoặc `include`, hoặc dùng một mẫu chặt hơn, nếu bạn cần \
             phần giữa ngay trong kết quả."
                .to_string()
        });

        // The cap notice is appended **after** folding, not before.
        //
        // Appended before, it lands in the middle section that gets cut, and the model
        // receives a truncated list that looks exactly like a complete one — precisely the
        // mistake this cap exists to warn about. A warning swallowed by the very mechanism
        // it describes is worse than no warning.
        let mut content = folded.content;
        match stopped {
            Some(Stopped::MatchCap) => content.push_str(&format!(
                "\n[đã dừng ở {MATCH_CAP} khớp — kho này còn khớp nữa mà việc tìm chưa đi \
                 tới. Hãy thu hẹp bằng `path` hoặc `include`, hoặc dùng mẫu chặt hơn.]"
            )),
            Some(Stopped::Deadline) => content.push_str(&format!(
                "\n[đã dừng sau {} giây — việc đi cây chưa hết. Hãy thu hẹp bằng `path` \
                 hoặc `include`.]",
                SEARCH_DEADLINE.as_secs()
            )),
            None => {}
        }

        let meta = json!({
            "shape": "matches",
            "truncated": hits.len() > DISPLAY_CAP || folded.truncated || stopped.is_some(),
            "total": hits.len(),
            "groups": groups,
        });
        let mut outcome = ToolOutcome::ok(content).with_meta("search", meta);
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
        Ok(outcome)
    }
}
