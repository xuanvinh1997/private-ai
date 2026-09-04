//! Wires the terminal into the tree: provide the host, register six tools, route `terminal_open`
//! through user approval as middleware (guards can only deny, never ask), and close every session
//! on teardown via `defer_async`, since closing must wait for the process tree to die.

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

/// Route every `terminal_open` past the user. Only that tool: approval buys the shell, and re-asking per keystroke teaches click-through.
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
            // Delegate first, then ask: if a lower layer already denied, there is nothing left to ask about.
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
    /// The prompt must state the real risk on this machine, not what the policy claims; same wording as `pai-shell` plus session persistence.
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

/// Register the six tools for one owner. Public so a sub-agent can share the same host under a different `owner`.
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
        // `workspace-write` by default, as in `pai-shell`: a coding agent must edit the repo and nothing outside it.
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
