//! `grep` — tìm nội dung.
//!
//! Dùng thẳng `grep-searcher` + `grep-regex` + `ignore`, tức là chính ruột của ripgrep
//! dưới dạng thư viện. Không spawn tiến trình nào, nên không phải đóng gói một binary
//! ngoài rồi nuôi nó qua từng bản phát hành, và không phụ thuộc vào việc máy người dùng
//! có `rg` hay không. Đây là chỗ Rust thắng đậm nhất so với bản Python.

use std::path::Path;

use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::path::FileRoots;

/// Bao nhiêu khớp được hiện. Phần dư nằm trong content và đi vào kho tràn, không mất.
const DISPLAY_CAP: usize = 250;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepArgs {
    /// Biểu thức chính quy, cú pháp Rust regex.
    pub pattern: String,
    /// Thư mục hoặc tệp để tìm. Bỏ trống là gốc workspace.
    pub path: Option<String>,
    /// Lọc theo tên tệp, ví dụ `*.rs`.
    pub include: Option<String>,
}

pub struct Grep {
    roots: FileRoots,
}

impl Grep {
    pub const NAME: &'static str = "grep";

    pub fn new(roots: FileRoots) -> Grep {
        Grep { roots }
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
             những gì `.gitignore` loại trừ.",
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

        let hits = tokio::task::spawn_blocking(move || -> Result<Vec<Hit>, String> {
            let mut walk = WalkBuilder::new(&base);
            if let Some(pattern) = &include {
                let mut overrides = OverrideBuilder::new(&base);
                overrides.add(pattern).map_err(|e| e.to_string())?;
                walk.overrides(overrides.build().map_err(|e| e.to_string())?);
            }

            let mut searcher = SearcherBuilder::new()
                // Gặp byte không thì bỏ cả tệp: một tệp nhị phân khớp regex sẽ nhả ra
                // hàng nghìn dòng rác và đẩy mọi kết quả thật ra khỏi tầm nhìn.
                .binary_detection(BinaryDetection::quit(0))
                .line_number(true)
                .build();

            let mut hits = Vec::new();
            for entry in walk.build().flatten() {
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
                        Ok(true)
                    }),
                );
            }
            Ok(hits)
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

        // Gom theo tệp cho phần hiển thị: đọc mười khớp trong một tệp dễ hơn mười dòng
        // rời rạc lặp lại cùng một đường dẫn.
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

        let meta = json!({
            "shape": "matches",
            "truncated": hits.len() > DISPLAY_CAP,
            "total": hits.len(),
            "groups": groups,
        });
        Ok(ToolOutcome::ok(rendered).with_meta("search", meta))
    }
}
