//! Nén ngữ cảnh.
//!
//! Khi lịch sử sắp không vừa cửa sổ của mô hình, phần cũ được **che** bằng một bản tóm
//! tắt. Ba quyết định đáng viết ra:
//!
//! **Che, không xoá.** Dải cũ vẫn nằm nguyên trong sổ và vẫn phát lại được; chỉ phép
//! chiếu ngừng nhìn thấy nó. Một bản ghi mất đoạn thì không ai dựng lại được lượt đã
//! chạy, kể cả chính ta lúc đi tìm nguyên nhân một câu trả lời sai.
//!
//! **Giữ lại phần đuôi.** Những lượt gần nhất là thứ mô hình đang thật sự làm việc trên
//! đó. Nén cả đuôi thì tiết kiệm được token và mất luôn mạch việc.
//!
//! **Không cắt giữa một lời gọi tool và kết quả của nó.** Một `tool_use` không có
//! `tool_result` đi kèm là lỗi giao thức ở cả hai nhà cung cấp — request bị từ chối thẳng,
//! và triệu chứng ("400 từ máy chủ") không hề chỉ về đây.
//!
//! Bản tóm tắt hiện được dựng bằng cách rút gọn cơ học, **không** gọi mô hình. Gọi mô
//! hình để tóm tắt là đúng hướng nhưng nó biến một chính sách tất định thành một lần gọi
//! mạng có thể hỏng ngay giữa lúc ngữ cảnh đã đầy — tức là hỏng đúng lúc tệ nhất. Khi có
//! bản tóm tắt bằng mô hình, nó sẽ là một provider phía sau seam này, và bản cơ học ở đây
//! vẫn là chỗ lùi về.

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use pai_session::{ContentBlock, Message, Role};

use crate::events::{PreStep, PreStepRequest, Replacement, StepDecision};

/// Vượt tỉ lệ này của cửa sổ thì bắt đầu nén.
const PRESSURE: f32 = 0.8;
/// Giữ lại chừng này cuối lịch sử, tính theo tỉ lệ cửa sổ.
const TAIL: f32 = 0.16;
/// Bốn ký tự một token. Thô, nhưng sai theo hướng an toàn với tiếng Việt, vốn tốn token
/// hơn tiếng Anh — nên ta nén sớm hơn cần thiết chứ không muộn hơn.
const CHARS_PER_TOKEN: usize = 4;

fn cost(message: &Message) -> usize {
    message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolCall {
                name, arguments, ..
            } => name.len() + arguments.len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
        })
        .sum::<usize>()
        / CHARS_PER_TOKEN
}

/// Một node có phải là lời gọi tool đang chờ kết quả không.
fn opens_tool_call(message: &Message) -> bool {
    message.role == Role::Assistant
        && message
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolCall { .. }))
}

/// Đẩy ranh giới cắt lùi lại cho tới khi nó không nằm giữa một cặp gọi/kết quả.
fn safe_boundary(history: &[Message], mut end: usize) -> usize {
    while end > 0 {
        let previous = &history[end - 1];
        // Cắt ngay sau một lời gọi tool sẽ để lại lời gọi mà bỏ kết quả — và ngược lại,
        // cắt ngay trước một kết quả sẽ để lại kết quả mồ côi. Cả hai đều bị máy chủ từ
        // chối, nên lùi thêm một node cho tới khi ranh giới sạch.
        let orphan_call = opens_tool_call(previous);
        let orphan_result = history.get(end).is_some_and(|next| next.role == Role::Tool);
        if !orphan_call && !orphan_result {
            break;
        }
        end -= 1;
    }
    end
}

fn summarize(dropped: &[Message]) -> Message {
    let users = dropped.iter().filter(|m| m.role == Role::User).count();
    let tools: Vec<&str> = dropped
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolCall { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut unique: Vec<&str> = tools.clone();
    unique.sort_unstable();
    unique.dedup();

    let tail = dropped
        .iter()
        .rev()
        .filter(|m| m.role == Role::User)
        .find_map(|m| {
            m.content.iter().find_map(|b| match b {
                ContentBlock::Text { text } => Some(text.chars().take(300).collect::<String>()),
                _ => None,
            })
        })
        .unwrap_or_default();

    let mut summary = format!(
        "<ngu-canh-da-nen>\nPhần đầu cuộc trò chuyện đã được rút gọn để vừa cửa sổ ngữ \
         cảnh: {} lượt của người dùng, {} lần gọi công cụ ({}).",
        users,
        tools.len(),
        if unique.is_empty() {
            "không có".to_string()
        } else {
            unique.join(", ")
        }
    );
    if !tail.is_empty() {
        summary.push_str(&format!("\nYêu cầu gần nhất trong phần đã rút gọn: {tail}"));
    }
    summary.push_str(
        "\nNếu cần chi tiết đã bị rút gọn, hãy hỏi lại người dùng thay vì suy đoán.\n\
         </ngu-canh-da-nen>",
    );

    Message {
        role: Role::User,
        content: vec![ContentBlock::Text { text: summary }],
        source: Some("compaction".into()),
    }
}

struct Compactor {
    /// Cửa sổ ngữ cảnh, tính bằng token.
    window: usize,
}

impl Middleware<PreStep> for Compactor {
    fn call<'a>(
        &'a self,
        req: &'a mut PreStepRequest,
        next: Next<'a, PreStep>,
    ) -> BoxFuture<'a, StepDecision> {
        async move {
            let used: usize = req.history.iter().map(cost).sum();
            if used < (self.window as f32 * PRESSURE) as usize {
                return next.run(req).await;
            }

            let keep = (self.window as f32 * TAIL) as usize;
            let mut tail_cost = 0usize;
            let mut end = req.history.len();
            while end > 0 && tail_cost < keep {
                end -= 1;
                tail_cost += cost(&req.history[end]);
            }
            let end = safe_boundary(&req.history, end);
            if end == 0 {
                // Không cắt được chỗ nào an toàn. Chạy tiếp và để máy chủ nói không —
                // một lỗi nói rõ vẫn tốt hơn một bản ghi bị cắt bậy.
                tracing::warn!(
                    used,
                    window = self.window,
                    "không tìm được ranh giới nén an toàn"
                );
                return next.run(req).await;
            }

            let summary = summarize(&req.history[..end]);
            let decision = next.run(req).await;
            match decision {
                // Bám vào quyết định của tầng dưới thay vì tự dựng lại: một listener khác
                // có thể đã sửa danh sách message, và dựng lại là xoá bản sửa của họ.
                StepDecision::Enter {
                    messages,
                    replace: None,
                } => StepDecision::Enter {
                    messages,
                    replace: Some(Replacement {
                        start: 0,
                        end,
                        summary,
                    }),
                },
                decided => decided,
            }
        }
        .boxed()
    }
}

pub struct CompactionPlugin {
    window: usize,
}

impl CompactionPlugin {
    /// `window` là cửa sổ ngữ cảnh của mô hình, tính bằng token.
    pub fn new(window: usize) -> CompactionPlugin {
        CompactionPlugin {
            window: window.max(2048),
        }
    }
}

#[async_trait]
impl Plugin for CompactionPlugin {
    fn name(&self) -> &'static str {
        "compaction"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        // Chạy **trước** mọi tầng khác: đo áp lực trên lịch sử chưa ai đụng vào, và một
        // listener khác thêm ngữ cảnh sau đó thì phần thêm nằm ngoài dải bị che.
        ctx.keep(ctx.on_waterfall_first(Arc::new(Compactor {
            window: self.window,
        })));
        Ok(())
    }
}
