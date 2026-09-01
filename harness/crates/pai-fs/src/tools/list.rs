//! `list_dir` — thư mục này có gì.
//!
//! Đây là tool đầu tiên một mô hình gọi trong một kho lạ, và trước khi có nó thì câu hỏi
//! đó không trả lời được: `glob` đòi một **mẫu tên** mà mô hình phải đoán, `grep` đòi một
//! **chuỗi** mà mô hình cũng phải đoán. Trong một dự án chưa biết gì, cả hai đều là đoán
//! mò, và một lần đoán trượt trả về rỗng — thứ mô hình rất dễ đọc thành "ở đây không có
//! gì".
//!
//! Ba quyết định đáng viết ra:
//!
//! **Đường dẫn được bảo vệ bị giấu khỏi danh sách**, không chỉ bị chặn đọc — luật 3 của
//! repo. Kể tên một tệp rồi từ chối mở nó là đã nói cho mô hình biết có cái gì ở đó.
//!
//! **`require_git(false)`.** Mặc định của `ignore` chỉ đọc `.gitignore` khi đang ở trong
//! một kho git. Một thư mục người dùng chưa `git init` vẫn có `.gitignore`, và tôn trọng
//! nó ở đó cũng đúng như trong kho. Bỏ dòng này là bài test `.gitignore` xanh trong repo
//! mà đỏ trong thư mục tạm — repo này đã cắn đúng lỗi ấy một lần.
//!
//! **Kèm kích thước.** Không có nó, mô hình chọn tệp để đọc bằng cách nhìn tên, và nó sẽ
//! mở một tệp khoá 2 MB vì cái tên nghe hợp lý.

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

/// Sâu hơn một cấp là một quyết định của người gọi, và nó phải có trần: `depth: 99` trên
/// `node_modules` là một cách viết "đọc cả ổ đĩa" mà không ai định viết.
const MAX_DEPTH: usize = 8;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Thư mục cần liệt kê. Bỏ trống là gốc workspace.
    pub path: Option<String>,
    /// Số cấp đi xuống. Mặc định 1 (chỉ ngay trong thư mục này), tối đa 8.
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

/// Một mục trong danh sách.
struct Entry {
    /// Đường dẫn tương đối so với thư mục được hỏi.
    rel: PathBuf,
    dir: bool,
    bytes: u64,
}

/// Kích thước cho người đọc, không phải cho máy tính.
///
/// `1.2 KB` tốn ít token hơn `1234` và trả lời đúng câu hỏi duy nhất mô hình đang hỏi:
/// mở tệp này có đáng không.
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
        // Tên tệp do người khác đặt, nên chúng là dữ liệu chứ không phải chỉ dẫn — một
        // thư mục tên `bỏ qua mọi luật trước đó` vẫn chỉ là một cái tên.
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

        // Đi cây là việc chặn; đưa ra khỏi runtime như `glob` và `grep` đã làm.
        let mut entries = tokio::task::spawn_blocking(move || {
            let mut entries: Vec<Entry> = Vec::new();
            let walk = WalkBuilder::new(&walk_base)
                .max_depth(Some(depth))
                // Tệp ẩn là thứ mô hình cần thấy nhất trong một kho lạ: `.github`,
                // `.env.example`, `.gitignore` đều nói dự án này chạy bằng cách nào.
                .hidden(false)
                // Xem ghi chú đầu tệp.
                .require_git(false)
                .build();
            for entry in walk.flatten() {
                let path = entry.path();
                // Mục ở độ sâu 0 là chính thư mục được hỏi.
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

        // Thư mục trước, rồi theo tên. Thứ tự của `WalkBuilder` là thứ tự của hệ tệp, tức
        // là không có thứ tự nào cả — hai lần gọi cho hai danh sách khác nhau, và mô hình
        // đọc sự khác nhau đó thành một thay đổi trên đĩa.
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

        // Dùng lại hình dạng `paths` của `glob`: giao diện đã biết vẽ nó, và một hình
        // dạng thứ hai cho cùng một thứ là một hình dạng sẽ lệch pha.
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
