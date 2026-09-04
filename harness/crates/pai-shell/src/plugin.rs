//! Mount the shell into the tree: provide the executor, register four tools, and attach the
//! guard that routes `bash` through the user. Disposing the plugin kills every background job.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Plugin};
use pai_core::{Middleware, Next};
use pai_tools::{Overflow, PreDecision, PreExecute, PreRequest, Tools};

use crate::jobs::Jobs;
use crate::provider::{LocalShell, Shell, ShellExecutor};
use crate::tools::bash::Bash;
use crate::tools::job::{JobKill, JobList, JobOutput};
use pai_sandbox::Policy;

/// Route every `bash` call through the user; middleware, since guards cannot open an ask.
struct AskBeforeShell {
    ctx: Context,
}

impl Middleware<PreExecute> for AskBeforeShell {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        async move {
            if req.name.as_str() != Bash::NAME {
                return next.run(req).await;
            }
            // Delegate first, ask second: a layer below may already have denied the call.
            match next.run(req).await {
                // The question must state the real risk; this is the line the user reads.
                PreDecision::Allow => PreDecision::Ask {
                    reason: self.risk(),
                },
                decided => decided,
            }
        }
        .boxed()
    }
}

impl AskBeforeShell {
    fn risk(&self) -> String {
        match self
            .ctx
            .get::<pai_sandbox::Sandbox>()
            .map(|s| s.enforcement())
        {
            Some(pai_sandbox::Enforcement::Full) => "Lệnh chạy trong vòng giam: nó chỉ ghi \
                 được vào thư mục làm việc. Nó vẫn đọc được toàn máy và vẫn ra được mạng."
                .into(),
            Some(other) => format!(
                "Vòng giam không đầy đủ trên máy này ({}). Lệnh chạy gần như với đầy đủ \
                 quyền của bạn.",
                other.reason().unwrap_or("không rõ lý do")
            ),
            None => "Không có vòng giam nào. Lệnh chạy với đầy đủ quyền của bạn.".into(),
        }
    }
}

pub struct ShellPlugin {
    cwd: PathBuf,
}

impl ShellPlugin {
    pub fn new(cwd: PathBuf) -> ShellPlugin {
        ShellPlugin { cwd }
    }
}

#[async_trait]
impl Plugin for ShellPlugin {
    fn name(&self) -> &'static str {
        "shell"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        // `workspace-write` by default: edit the repo and nothing outside it.
        let policy = Policy::workspace_write(self.cwd.clone());
        let shell: Arc<dyn ShellExecutor> = Arc::new(LocalShell::new(ctx.clone(), policy));
        ctx.keep(ctx.provide::<Shell>(shell.clone())?);

        let jobs = Arc::new(Jobs::default());
        {
            let jobs = jobs.clone();
            ctx.effects().defer("jobs", move || jobs.kill_all());
        }

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(Bash::new(
            shell,
            jobs.clone(),
            self.cwd.clone(),
            Overflow::new(ctx),
        ))));
        ctx.keep(tools.register(Arc::new(JobOutput::new(jobs.clone()))));
        ctx.keep(tools.register(Arc::new(JobKill::new(jobs.clone()))));
        ctx.keep(tools.register(Arc::new(JobList::new(jobs))));
        ctx.keep(ctx.on_waterfall(Arc::new(AskBeforeShell { ctx: ctx.clone() })));
        Ok(())
    }
}
