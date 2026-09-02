//! Mount the shell into the tree.
//!
//! The plugin does three things, and the third is the one that matters: provide the
//! executor, register four tools, and attach a guard that routes `bash` through the ask-
//! the-user path. Without that guard, `bash` is a tool that runs anything with nobody
//! given a chance to say no.
//!
//! Disposing the plugin kills every background job. A process that outlives the thing that
//! spawned it is a process nobody remembers to clean up.

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

/// Route every `bash` call through the user.
///
/// A middleware rather than a guard, because guards are deliberately **monotonic**: they
/// only deny or abstain, and cannot open an ask. That is the right design — if a guard
/// could return `Ask`, registration order would turn a denial into a question, and a
/// question can be answered yes.
///
/// A separate plugin rather than a field on `ToolMeta`, because the policy has to be
/// removable: a headless build with a real sandbox will swap it for something else, and
/// nobody should have to edit `bash` to do that.
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
            // Delegate first, ask second: if a layer below already denied, there is
            // nothing to ask about, and asking a question whose answer changes nothing
            // trains the user to click straight through.
            match next.run(req).await {
                // The question has to state the real level of risk. This is the exact
                // line the user reads before clicking "allow", so anything vague here is
                // a line that trains them to click through.
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
        // `workspace-write` is the default: a coding agent has to be able to edit the
        // repo, and nothing outside it. Tighter or looser is a per-session decision.
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
