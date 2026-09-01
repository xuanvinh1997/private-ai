//! `grep` — tìm nội dung.
//!
//! Dùng thẳng `grep-searcher` + `grep-regex` + `ignore`, tức là chính ruột của ripgrep
//! dưới dạng thư viện. Không spawn tiến trình nào, nên không phải đóng gói một binary
//! ngoài rồi nuôi nó qua từng bản phát hành, và không phụ thuộc vào việc máy người dùng
//! có `rg` hay không. Đây là chỗ Rust thắng đậm nhất so với bản Python.

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

/// Bao nhiêu khớp được gom vào `meta` cho giao diện vẽ.
const DISPLAY_CAP: usize = 250;

/// Trần cứng số khớp thu thập.
///
/// Không có nó, một mẫu như `.` trên một kho vài trăm nghìn tệp gom hàng chục triệu dòng
/// vào bộ nhớ trước khi có ai kịp nói gì. Trần này chặn ở chỗ *thu thập*, không phải chỗ
/// hiển thị: dừng đi cây luôn, chứ không quét xong rồi mới vứt.
const MATCH_CAP: usize = 5_000;

/// Trần thời gian đi cây.
///
/// Một kho lớn trên ổ mạng có thể quét lâu hơn cả hạn giờ của tool, và lúc đó mô hình
/// nhận về một dòng "quá 120 giây" thay vì những khớp đã tìm được. Kết quả một phần kèm
/// lời nói rõ nó là một phần thì có ích; im lặng hết giờ thì không.
const SEARCH_DEADLINE: Duration = Duration::from_secs(20);

/// Vì sao việc đi cây dừng sớm.
#[derive(Clone, Copy)]
enum Stopped {
    MatchCap,
    Deadline,
}

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
                    // Gặp byte không thì bỏ cả tệp: một tệp nhị phân khớp regex sẽ nhả ra
                    // hàng nghìn dòng rác và đẩy mọi kết quả thật ra khỏi tầm nhìn.
                    .binary_detection(BinaryDetection::quit(0))
                    .line_number(true)
                    .build();

                let started = Instant::now();
                let mut hits: Vec<Hit> = Vec::new();
                let mut stopped = None;
                for entry in walk.build().flatten() {
                    // Trần số khớp dừng việc đi cây, nhưng **không** kết luận ở đây: nó
                    // được kết luận sau vòng lặp, vì một tệp duy nhất cũng có thể tự nó
                    // chạm trần và lúc đó vòng lặp kết thúc mà chưa lần nào chạy tới đây.
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
                            // `false` dừng ngay tệp này: một tệp sinh mã có thể một mình
                            // vượt cả trần.
                            Ok(hits.len() < MATCH_CAP)
                        }),
                    );
                }
                // Chạm trần là chạm trần, dù vòng lặp dừng vì trần hay vì hết tệp: khi
                // đã đủ `MATCH_CAP` khớp thì không còn cách nào biết ngoài kia còn gì.
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

        let folded = self.overflow.fold(&call.name, rendered, |_| {
            "Thu hẹp bằng `path` hoặc `include`, hoặc dùng một mẫu chặt hơn, nếu bạn cần \
             phần giữa ngay trong kết quả."
                .to_string()
        });

        // Lời báo chạm trần nối vào **sau** khi gấp, không phải trước.
        //
        // Nối trước thì nó rơi đúng vào khúc giữa bị cắt đi, và mô hình nhận về một danh
        // sách cụt trông y hệt một danh sách đầy đủ — đúng cái lỗi mà trần này sinh ra để
        // nói với nó. Một cảnh báo bị chính cơ chế cắt nuốt mất còn tệ hơn không có.
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
