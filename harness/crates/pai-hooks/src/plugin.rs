//! Cắm hook vào đường ống tool.

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use pai_tools::{PreDecision, PreExecute, PreRequest};
use serde::Deserialize;

use crate::runner::{HookDecision, HookInput, run};

/// Một hook trong tệp cấu hình.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookConfig {
    /// Lệnh chạy qua `/bin/sh -c`.
    pub command: String,
    /// Chỉ chạy cho những tool này. Rỗng = mọi tool.
    ///
    /// Lọc ở đây chứ không để hook tự lọc, vì mỗi lần gọi hook là một lần spawn tiến
    /// trình — một hook chỉ quan tâm `bash` mà bị gọi cho từng lần `read` sẽ làm chậm
    /// đúng những lời gọi rẻ nhất.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Hạn giờ riêng cho hook này, tính bằng giây. Vắng thì dùng [`HOOK_TIMEOUT`].
    ///
    /// Có mặt vì hai lý do khác nhau và cả hai đều thật. Thứ nhất, một hook gọi ra mạng
    /// cần dài hơn một hook chạy `grep`, và một con số chung cho cả hai thì sai với ít
    /// nhất một bên. Thứ hai, **bộ test cần rút ngắn nó**: bài kiểm hạn giờ mà đo đồng hồ
    /// thật sẽ đỏ ngẫu nhiên khi máy đang chạy hai chục bài khác song song — đó là chuyện
    /// đã xảy ra hai lần ở kho này.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl HookConfig {
    fn applies_to(&self, tool: &str) -> bool {
        self.tools.is_empty() || self.tools.iter().any(|name| name == tool)
    }
}

struct PreHooks {
    hooks: Vec<HookConfig>,
}

impl Middleware<PreExecute> for PreHooks {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        async move {
            let tool = req.name.as_str().to_string();
            for hook in self.hooks.iter().filter(|hook| hook.applies_to(&tool)) {
                let input = HookInput {
                    event: "pre-execute",
                    tool: &tool,
                    call_id: &req.call_id,
                    arguments: &req.arguments,
                    output: None,
                };
                // Một hook nói "không" thì dừng ngay, không hỏi những hook còn lại: câu
                // trả lời đã có, và chạy tiếp chỉ tốn thêm tiến trình.
                // Hạn giờ của chính hook này, hoặc mặc định. Xem `HookConfig::timeout_secs`.
                let deadline = hook
                    .timeout_secs
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(crate::runner::HOOK_TIMEOUT);
                if let Some(HookDecision::Deny { reason }) =
                    run(&hook.command, &input, deadline).await
                {
                    return PreDecision::Deny(reason);
                }
            }
            next.run(req).await
        }
        .boxed()
    }
}

pub struct HooksPlugin {
    hooks: Vec<HookConfig>,
}

impl HooksPlugin {
    pub fn new(hooks: Vec<HookConfig>) -> HooksPlugin {
        HooksPlugin { hooks }
    }
}

#[async_trait]
impl Plugin for HooksPlugin {
    fn name(&self) -> &'static str {
        "hooks"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        if self.hooks.is_empty() {
            return Ok(());
        }
        // Chạy **trước** mọi tầng khác, kể cả trước phê duyệt: chính sách của người vận
        // hành không nên phải chờ người dùng trả lời một câu hỏi về việc mà chính sách đã
        // quyết định là không được làm.
        ctx.keep(ctx.on_waterfall_first(Arc::new(PreHooks {
            hooks: self.hooks.clone(),
        })));
        Ok(())
    }
}
