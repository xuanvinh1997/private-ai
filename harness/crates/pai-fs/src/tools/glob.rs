//! `glob` — tìm tệp theo mẫu tên.
//!
//! Hai chi tiết nhỏ quyết định mô hình dùng đúng hay sai, nên viết ra đây: kết quả **chỉ
//! có tệp, không có thư mục**, và một mẫu không chứa `/` khớp **tên tệp ở mọi độ sâu**.
//! Không có luật thứ hai thì `*.rs` trả về rỗng ở mọi repo có thư mục con, và mô hình
//! kết luận sai rằng repo không có tệp Rust nào.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::path::FileRoots;

/// Bao nhiêu đường dẫn được hiện ra. Phần dư không mất — nó nằm trong content, và đường
/// ống cất content dài vào kho tràn.
const DISPLAY_CAP: usize = 100;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// Mẫu tên tệp, ví dụ `*.rs` hoặc `src/**/*.ts`.
    pub pattern: String,
    /// Thư mục bắt đầu. Bỏ trống là gốc workspace.
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

/// Mẫu không có `/` thì khớp tên tệp; có `/` thì khớp đường dẫn tương đối từ gốc tìm.
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
        // Tên tệp do người dùng đặt, nên chúng cũng là dữ liệu chứ không phải chỉ dẫn.
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

        // Đi cây thư mục là việc chặn. Đưa nó ra khỏi runtime, nếu không một repo lớn sẽ
        // giữ luôn cả reactor trong lúc quét.
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
                // Giấu khỏi listing, không chỉ chặn đọc: kể tên một tệp được bảo vệ là
                // đã nói cho mô hình biết có cái gì ở đó để mà đi tìm đường khác.
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
            // Mới nhất trước: tệp vừa đụng gần như luôn là tệp đang được hỏi tới.
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
