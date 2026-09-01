//! `edit` — thay một đoạn văn bản nguyên văn.
//!
//! Khớp nguyên văn chứ không phải biểu thức chính quy, và mặc định phải khớp **đúng một
//! lần**. Cả hai đều là ràng buộc có chủ ý: một mẫu khớp nhiều chỗ mà chỉ định sửa một
//! chỗ là cách một lần sửa lan ra khắp tệp. Khi khớp nhiều lần, lỗi trả về phải nói rõ
//! *bao nhiêu* lần — đó là con số mô hình cần để quyết định nới đoạn trích hay bật
//! `replace_all`.

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
pub struct EditArgs {
    /// Đường dẫn tuyệt đối tới tệp cần sửa.
    pub file_path: String,
    /// Đoạn cần thay, nguyên văn kể cả thụt lề. Phải đủ dài để chỉ khớp một chỗ.
    pub old_string: String,
    /// Đoạn thay thế.
    pub new_string: String,
    /// Thay mọi chỗ khớp. Mặc định `false`.
    #[serde(default)]
    pub replace_all: bool,
}

pub struct Edit {
    fs: Arc<dyn FsProvider>,
    roots: FileRoots,
}

impl Edit {
    pub const NAME: &'static str = "edit";

    pub fn new(fs: Arc<dyn FsProvider>, roots: FileRoots) -> Edit {
        Edit { fs, roots }
    }
}

#[async_trait]
impl Tool for Edit {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Edit::NAME,
            "Thay một đoạn văn bản trong tệp. Khớp nguyên văn, kể cả khoảng trắng. \
             `old_string` phải khớp đúng một chỗ, trừ khi bật `replace_all`.",
            json_schema_for::<EditArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::mutating().concurrency_safe(false)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: EditArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        if args.old_string == args.new_string {
            return Err(ToolError::Invalid(
                "`old_string` và `new_string` giống hệt nhau; không có gì để sửa.".into(),
            ));
        }

        let resolved = self
            .roots
            .resolve_write(Path::new(&args.file_path))
            .map_err(|err| ToolError::Invalid(err.to_string()))?;
        let shown = resolved.display().to_string();

        let before = self
            .fs
            .read_text(&resolved)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let hits = before.matches(&args.old_string).count();
        match hits {
            // Không sửa gì cả trước khi báo lỗi: một lần `edit` hỏng phải để tệp y nguyên.
            0 => {
                return Err(ToolError::Invalid(format!(
                    "không tìm thấy đoạn cần thay trong {shown}. Hãy `read` lại tệp: nội \
                     dung có thể đã đổi, hoặc khoảng trắng không khớp."
                )));
            }
            n if n > 1 && !args.replace_all => {
                return Err(ToolError::Invalid(format!(
                    "đoạn cần thay khớp {n} chỗ trong {shown}. Hãy trích dài hơn để chỉ \
                     còn một chỗ, hoặc bật `replace_all` nếu thật sự muốn sửa cả {n}."
                )));
            }
            _ => {}
        }

        let after = if args.replace_all {
            before.replace(&args.old_string, &args.new_string)
        } else {
            before.replacen(&args.old_string, &args.new_string, 1)
        };

        self.fs
            .write_text(&resolved, &after)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;

        let replaced = if args.replace_all { hits } else { 1 };
        Ok(ToolOutcome::ok(format!("Đã sửa {shown} ({replaced} chỗ)."))
            .with_meta("diffs", diff::between(&shown, &before, &after)))
    }
}
