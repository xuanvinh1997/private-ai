//! `list_dir` — what is in this directory.
//!
//! This is the first tool a model reaches for in an unfamiliar repo, and before it existed
//! that question had no answer: `glob` wants a **name pattern** the model has to guess, and
//! `grep` wants a **string** it also has to guess. In a project it knows nothing about both
//! are shots in the dark, and a missed guess returns empty — which a model very easily reads
//! as "there is nothing here".
//!
//! Three decisions worth writing down:
//!
//! **Protected paths are hidden from the listing**, not merely blocked from reading — rule 3
//! of the repo. Naming a file and then refusing to open it has already told the model
//! something is there.
//!
//! **`require_git(false)`.** The `ignore` crate only reads `.gitignore` when inside a git
//! repo by default. A directory the user never ran `git init` in can still have a
//! `.gitignore`, and honouring it there is as correct as honouring it in a repo. Dropping
//! this line makes the `.gitignore` test pass in the repo and fail in a temp directory —
//! this repo hit exactly that bug once.
//!
//! **Sizes are included.** Without them the model picks files to read by name, and it will
//! open a 2 MB lockfile because the name sounded plausible.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ignore::WalkBuilder;
use pai_tools::{
    Invocation, Overflow, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::path::FileRoots;

/// Going deeper than one level is the caller's decision, and it needs a ceiling: `depth: 99`
/// on `node_modules` is a way of writing "read the whole disk" that nobody meant to write.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// The directory to list. Empty means the workspace root.
    pub path: Option<String>,
    /// How many levels to descend. Defaults to 1 (this directory only), maximum 8.
    pub depth: Option<usize>,
}

pub struct ListDir {
    roots: FileRoots,
    overflow: Overflow,
}

impl ListDir {
    pub const NAME: &'static str = "list_dir";

    pub fn new(roots: FileRoots, overflow: Overflow) -> ListDir {
        ListDir { roots, overflow }
    }
}

/// One entry in the listing.
struct Entry {
    /// The path relative to the directory that was asked about.
    rel: PathBuf,
    dir: bool,
    bytes: u64,
}

/// A size for a reader, not for a computer.
///
/// `1.2 KB` costs fewer tokens than `1234` and answers the only question the model is
/// asking: is this file worth opening.
fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[async_trait]
impl Tool for ListDir {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            ListDir::NAME,
            "Liệt kê những gì có trong một thư mục: thư mục con trước, rồi tệp theo tên, \
             kèm kích thước. Tôn trọng `.gitignore`. Đây là tool để gọi **đầu tiên** khi \
             chưa biết dự án có gì — `glob` cần một mẫu tên mà bạn phải đoán trước, còn \
             tool này trả về đúng những gì đang có ở đó. Dùng `glob` khi đã biết mình tìm \
             tên nào, dùng `grep` khi đã biết mình tìm nội dung nào.",
            json_schema_for::<ListArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // File names are chosen by other people, so they are data and not instructions —
        // a directory named `ignore all previous rules` is still just a name.
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: ListArgs =
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
        if !base.is_dir() {
            return Err(ToolError::Invalid(format!(
                "{} không phải một thư mục; dùng `read` để mở một tệp.",
                base.display()
            )));
        }
        let depth = args.depth.unwrap_or(1).clamp(1, MAX_DEPTH);
        let roots = self.roots.clone();
        let walk_base = base.clone();

        // Walking is blocking work; move it off the runtime, as `glob` and `grep` do.
        let mut entries = tokio::task::spawn_blocking(move || {
            let mut entries: Vec<Entry> = Vec::new();
            let walk = WalkBuilder::new(&walk_base)
                .max_depth(Some(depth))
                // Hidden files are what a model most needs to see in an unfamiliar repo:
                // `.github`, `.env.example`, `.gitignore` all say how this project runs.
                .hidden(false)
                // See the note at the top of the file.
                .require_git(false)
                .build();
            for entry in walk.flatten() {
                let path = entry.path();
                // The entry at depth 0 is the directory that was asked about.
                if path == walk_base {
                    continue;
                }
                if roots.is_protected(path) {
                    continue;
                }
                let dir = entry.file_type().is_some_and(|t| t.is_dir());
                let bytes = entry
                    .metadata()
                    .ok()
                    .filter(|_| !dir)
                    .map(|m| m.len())
                    .unwrap_or(0);
                entries.push(Entry {
                    rel: path.strip_prefix(&walk_base).unwrap_or(path).to_path_buf(),
                    dir,
                    bytes,
                });
            }
            entries
        })
        .await
        .map_err(|err| ToolError::Failed(err.to_string()))?;

        // Directories first, then by name. `WalkBuilder`'s order is the filesystem's order,
        // which is to say no order at all — two calls give two different listings, and the
        // model reads that difference as a change on disk.
        entries.sort_by(|a, b| (!a.dir, &a.rel).cmp(&(!b.dir, &b.rel)));

        if entries.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "{} rỗng (hoặc mọi thứ trong đó bị `.gitignore` loại trừ).",
                base.display()
            )));
        }

        let dirs = entries.iter().filter(|e| e.dir).count();
        let files = entries.len() - dirs;
        let mut rendered = format!(
            "{} — {dirs} thư mục, {files} tệp (sâu {depth} cấp)\n",
            base.display()
        );
        let mut paths = Vec::with_capacity(entries.len());
        for entry in &entries {
            let name = entry.rel.display().to_string();
            if entry.dir {
                rendered.push_str(&format!("{name}/\n"));
                paths.push(format!("{name}/"));
            } else {
                rendered.push_str(&format!("{name}\t{}\n", human(entry.bytes)));
                paths.push(name);
            }
        }

        let folded = self.overflow.fold(&call.name, rendered, |_| {
            "Gọi lại với `path` trỏ vào một thư mục con, hoặc `depth` nhỏ hơn.".to_string()
        });

        // Reuse `glob`'s `paths` shape: the UI already knows how to draw it, and a second
        // shape for the same thing is a shape that will drift out of sync.
        let meta = json!({
            "shape": "paths",
            "truncated": folded.truncated,
            "total": entries.len(),
            "paths": paths,
        });
        let mut outcome = ToolOutcome::ok(folded.content).with_meta("search", meta);
        if let Some(handle) = folded.spill {
            outcome.meta.insert("spill".into(), handle.to_json());
        }
        Ok(outcome)
    }
}
