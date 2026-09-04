//! Context compaction: old history is masked behind a summary, never deleted.
//! The tail is kept, and a cut never falls between a tool call and its result, which both
//! providers reject. The summary is mechanical so the policy stays deterministic.

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use pai_session::{ContentBlock, Message, Role};

use crate::events::{PreStep, PreStepRequest, Replacement, StepDecision};

/// Past this fraction of the window, compaction starts.
const PRESSURE: f32 = 0.8;
/// Keep this much of the tail, as a fraction of the window.
const TAIL: f32 = 0.16;
/// Four characters per token: crude, but it errs early for Vietnamese, which costs more tokens than English.
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

/// Whether a node is a tool call still awaiting its result.
fn opens_tool_call(message: &Message) -> bool {
    message.role == Role::Assistant
        && message
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolCall { .. }))
}

/// Move the cut boundary back until it no longer falls between a call and its result.
fn safe_boundary(history: &[Message], mut end: usize) -> usize {
    while end > 0 {
        let previous = &history[end - 1];
        // Either side of the pair left alone is an orphan the server rejects, so step back until the boundary is clean.
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
    /// The context window, in tokens.
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
                // No safe cut exists; go on and let the server say no, which beats a badly cut record.
                tracing::warn!(
                    used,
                    window = self.window,
                    "no safe compaction boundary found"
                );
                return next.run(req).await;
            }

            let summary = summarize(&req.history[..end]);
            let decision = next.run(req).await;
            match decision {
                // Build on the inner decision rather than remaking it: another listener may have edited the messages.
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
    /// `window` is the model's context window, in tokens.
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
        // Run before every other layer, so pressure is measured on untouched history and later additions stay unmasked.
        ctx.keep(ctx.on_waterfall_first(Arc::new(Compactor {
            window: self.window,
        })));
        Ok(())
    }
}
