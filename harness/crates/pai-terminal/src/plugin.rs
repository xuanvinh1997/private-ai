//! Cắm terminal vào cây.
//!
//! Plugin làm ba việc, và việc thứ ba là việc quan trọng: cung cấp provider, đăng ký sáu
//! tool, và đẩy `terminal_open` qua đường hỏi người dùng.
//!
//! Việc hỏi là một **middleware** chứ không phải một canh gác, cùng lý do như
//! `pai-shell::plugin`: canh gác cố ý đơn điệu — chúng chỉ từ chối hoặc bỏ qua, không mở
//! được đường hỏi. Nếu canh gác trả `Ask` được thì thứ tự đăng ký sẽ biến một lệnh từ chối
//! thành một câu hỏi, mà một câu hỏi thì trả lời "có" được.
//!
//! Và gỡ plugin **đóng sạch mọi phiên**, kể cả tiến trình cháu. Một shell sống lâu hơn thứ
//! sinh ra nó là một shell không ai còn nhớ để dọn, còn nó thì vẫn giữ cổng và vẫn ghi vào
//! thư mục làm việc. Việc dọn dùng `defer_async` chứ không `defer`, vì lời hứa của
//! `terminal_close` là **chờ** cho cây tiến trình biến mất, và một disposer đồng bộ không
//! chờ được gì cả.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use pai_sandbox::Policy;
use pai_tools::{PreDecision, PreExecute, PreRequest, Tools};

use crate::provider::LocalTerminals;
use crate::seam::{DEFAULT_MAX_LINES, TerminalHost, Terminals};
use crate::tools::{
    TerminalClose, TerminalList, TerminalOpen, TerminalRead, TerminalSend, TerminalSignal,
};

/// Đẩy mọi lời gọi `terminal_open` qua người dùng.
///
/// Chỉ `terminal_open`, không phải cả sáu tool. Cái được duyệt là **quyền có một shell**;
/// một khi phiên đã mở với sự đồng ý của người dùng thì hỏi lại ở từng lần gõ phím là dạy
/// họ bấm cho qua — và một người bấm cho qua thì không đọc, kể cả lần hỏi đáng đọc.
struct AskBeforeOpen {
    ctx: Context,
}

impl Middleware<PreExecute> for AskBeforeOpen {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        async move {
            if req.name.as_str() != TerminalOpen::NAME {
                return next.run(req).await;
            }
            // Uỷ quyền trước rồi mới hỏi: một tầng dưới đã từ chối thì không còn gì để
            // hỏi, và hỏi một câu mà câu trả lời không đổi được gì là làm người dùng quen
            // với việc bấm cho qua.
            match next.run(req).await {
                PreDecision::Allow => PreDecision::Ask {
                    reason: self.risk(),
                },
                decided => decided,
            }
        }
        .boxed()
    }
}

impl AskBeforeOpen {
    /// Câu hỏi phải nói đúng mức rủi ro thật trên **máy đang chạy**, không phải mức rủi ro
    /// mà chính sách khai. Cùng chữ, cùng nguồn với `pai-shell`, cộng một câu về chuyện
    /// phiên sống lâu — vì đó là điều khác biệt duy nhất mà người đọc cần biết.
    fn risk(&self) -> String {
        let confinement = match self
            .ctx
            .get::<pai_sandbox::Sandbox>()
            .map(|sandbox| sandbox.enforcement())
        {
            Some(pai_sandbox::Enforcement::Full) => "Phiên chạy trong vòng giam: nó chỉ ghi \
                 được vào thư mục làm việc. Nó vẫn đọc được toàn máy và vẫn ra được mạng."
                .to_string(),
            Some(other) => format!(
                "Vòng giam không đầy đủ trên máy này ({}). Phiên chạy gần như với đầy đủ \
                 quyền của bạn.",
                other.reason().unwrap_or("không rõ lý do")
            ),
            None => "Không có vòng giam nào. Phiên chạy với đầy đủ quyền của bạn.".to_string(),
        };
        format!(
            "{confinement} Khác một lệnh lẻ, phiên này **ở lại**: mọi lần gọi sau chạy \
             trong nó mà không hỏi lại, cho tới khi nó được đóng."
        )
    }
}

pub struct TerminalPlugin {
    cwd: PathBuf,
    max_lines: usize,
}

impl TerminalPlugin {
    pub fn new(cwd: PathBuf) -> TerminalPlugin {
        TerminalPlugin {
            cwd,
            max_lines: DEFAULT_MAX_LINES,
        }
    }

    pub fn with_max_lines(mut self, lines: usize) -> TerminalPlugin {
        self.max_lines = lines;
        self
    }
}

/// Đăng ký sáu tool cho một chủ.
///
/// Tách khỏi [`Plugin::apply`] và để `pub`, vì đây là cách một agent con có bộ tool riêng
/// nhìn vào cùng một bể phiên mà **không** thấy phiên của agent khác: cùng `host`, khác
/// `owner`. Tự tay ráp lại ở chỗ gọi thì sớm muộn có chỗ ráp thiếu một tool, và cái thiếu
/// đó là một khả năng biến mất trong im lặng.
pub fn register_tools(
    ctx: &Context,
    host: Arc<dyn TerminalHost>,
    max_lines: usize,
) -> anyhow::Result<()> {
    let tools = ctx.require::<Tools>()?;
    let owner = ctx.scope_key();
    ctx.keep(tools.register(Arc::new(TerminalOpen::new(host.clone(), owner))));
    ctx.keep(tools.register(Arc::new(TerminalList::new(host.clone(), owner))));
    ctx.keep(tools.register(Arc::new(TerminalRead::new(host.clone(), owner, max_lines))));
    ctx.keep(tools.register(Arc::new(TerminalSend::new(host.clone(), owner, max_lines))));
    ctx.keep(tools.register(Arc::new(TerminalSignal::new(host.clone(), owner))));
    ctx.keep(tools.register(Arc::new(TerminalClose::new(host, owner))));
    Ok(())
}

#[async_trait]
impl Plugin for TerminalPlugin {
    fn name(&self) -> &'static str {
        "terminal"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        // `workspace-write` là mặc định, cùng lý do như `pai-shell`: một coding agent phải
        // sửa được repo, và mọi thứ ngoài repo thì không.
        let policy = Policy::workspace_write(self.cwd.clone());
        let terminals = Arc::new(
            LocalTerminals::new(ctx.clone(), policy, self.cwd.clone())
                .with_max_lines(self.max_lines),
        );
        let host: Arc<dyn TerminalHost> = terminals.clone();
        ctx.keep(ctx.provide::<Terminals>(host.clone())?);

        {
            let closing = terminals.clone();
            ctx.effects().defer_async(
                "terminals",
                move || async move { closing.close_all().await },
            );
        }

        register_tools(ctx, host, self.max_lines)?;
        ctx.keep(ctx.on_waterfall(Arc::new(AskBeforeOpen { ctx: ctx.clone() })));
        Ok(())
    }
}
