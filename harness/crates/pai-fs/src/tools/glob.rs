//! `glob`: find files by name pattern. Results are files only, never directories, and a
//! pattern without `/` matches the file name at any depth, or `*.rs` would return nothing
//! in any repo with subdirectories.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::path::FileRoots;

/// How many paths are shown; the rest stay in the content, which the pipeline may spill.
const DISPLAY_CAP: usize = 100;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// The file-name pattern, e.g. `*.rs` or `src/**/*.ts`.
    pub pattern: String,
    /// Where to start. Empty means the workspace root.
    pub path: Option<String>,
}

pub struct GlobTool {
    roots: FileRoots,
}

impl GlobTool {
    pub const NAME: &'static str = "glob";

    pub fn new(roots: FileRoots) -> GlobTool {
        GlobTool { roots }
    }
}

/// Without `/` the pattern matches the file name; with `/`, the path relative to the search root.
fn matcher(pattern: &str) -> Result<(GlobMatcher, bool), ToolError> {
    let by_name = !pattern.contains('/');
    let glob = Glob::new(pattern).map_err(|err| ToolError::Invalid(err.to_string()))?;
    Ok((glob.compile_matcher(), by_name))
}

#[async_trait]
impl Tool for GlobTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            GlobTool::NAME,
            "Liệt kê tệp khớp một mẫu tên. Chỉ trả về tệp, không trả về thư mục. Mẫu \
             không chứa `/` sẽ khớp tên tệp ở mọi độ sâu. Kết quả sắp theo lần sửa gần \
             nhất trước.",
            json_schema_for::<GlobArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // File names are chosen by the user, so they are data too, not instructions.
        ToolMeta::read_only().untrusted().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: GlobArgs =
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
        let (glob, by_name) = matcher(&args.pattern)?;
        let roots = self.roots.clone();

        // Walking the tree blocks, so keep it off the runtime or a large repo stalls the reactor.
        let found = tokio::task::spawn_blocking(move || {
            let mut hits: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
            for entry in WalkBuilder::new(&base).hidden(false).build().flatten() {
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let path = entry.path();
                let candidate: &Path = if by_name {
                    Path::new(path.file_name().unwrap_or_default())
                } else {
                    path.strip_prefix(&base).unwrap_or(path)
                };
                if !glob.is_match(candidate) {
                    continue;
                }
                // Hidden, not just unreadable: naming a protected file already leaks its existence.
                if roots.is_protected(path) {
                    continue;
                }
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                hits.push((path.to_path_buf(), mtime));
            }
            // Most recent first: the file just touched is almost always the one being asked about.
            hits.sort_unstable_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
            hits.into_iter().map(|(path, _)| path).collect::<Vec<_>>()
        })
        .await
        .map_err(|err| ToolError::Failed(err.to_string()))?;

        if found.is_empty() {
            return Ok(ToolOutcome::ok(format!(
                "Không có tệp nào khớp `{}` dưới {}.",
                args.pattern,
                base_display(&args)
            )));
        }

        let listed: Vec<String> = found.iter().map(|p| p.display().to_string()).collect();
        let shown: Vec<&String> = listed.iter().take(DISPLAY_CAP).collect();
        let meta = json!({
            "shape": "paths",
            "truncated": listed.len() > DISPLAY_CAP,
            "total": listed.len(),
            "paths": shown,
        });

        Ok(ToolOutcome::ok(listed.join("\n")).with_meta("search", meta))
    }
}

fn base_display(args: &GlobArgs) -> String {
    args.path.clone().unwrap_or_else(|| "workspace".to_string())
}
