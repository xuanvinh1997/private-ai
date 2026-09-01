//! `write` — ghi đè cả một tệp.
//!
//! Tách khỏi `edit` vì hai việc khác nhau: `write` dựng một tệp, `edit` sửa một chỗ. Gộp
//! lại thì mô hình sẽ dùng `write` cho việc sửa, và mỗi lần sửa một dòng là một lần chép
//! lại cả tệp từ trí nhớ — nơi mọi thứ nó không nhớ sẽ biến mất.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolSchema, json_schema_for};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::path::FileRoots;
use crate::provider::FsProvider;
use crate::tools::diff;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Đường dẫn tuyệt đối. Thư mục cha được tạo nếu chưa có.
    pub file_path: String,
    /// Toàn bộ nội dung mới của tệp.
    pub content: String,
}

pub struct Write {
    fs: Arc<dyn FsProvider>,
    roots: FileRoots,
}

impl Write {
    pub const NAME: &'static str = "write";

    pub fn new(fs: Arc<dyn FsProvider>, roots: FileRoots) -> Write {
        Write { fs, roots }
    }
}

#[async_trait]
impl Tool for Write {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Write::NAME,
            "Ghi toàn bộ nội dung một tệp, tạo mới hoặc ghi đè. Để sửa một phần của tệp \
             đã có, dùng `edit` chứ không dùng tool này.",
            json_schema_for::<WriteArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Hai lần ghi song song lên cùng một tệp thì một trong hai biến mất, và không có
        // cách nào biết là cái nào.
        ToolMeta::mutating().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: WriteArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let resolved = self
            .roots
            .resolve_write(Path::new(&args.file_path))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let shown = resolved.display().to_string();

        let existed = self.fs.exists(&resolved).await;
        let before = if existed {
            self.fs.read_text(&resolved).await.unwrap_or_default()
        } else {
            String::new()
        };

        self.fs
            .write_text(&resolved, &args.content)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let diffs = if existed {
            diff::between(&shown, &before, &args.content)
        } else {
            diff::created(&shown, &args.content)
        };
        let verb = if existed {
            "Đã ghi đè"
        } else {
            "Đã tạo"
        };
        let lines = args.content.lines().count();

        Ok(ToolOutcome::ok(format!("{verb} {shown} ({lines} dòng).")).with_meta("diffs", diffs))
    }
}
