//! Cắm shell vào cây.
//!
//! Plugin làm ba việc, và việc thứ ba mới là việc quan trọng: cung cấp provider, đăng ký
//! bốn tool, và gắn một canh gác đẩy `bash` qua đường hỏi người dùng. Không có canh gác
//! đó thì `bash` là một tool chạy được bất cứ thứ gì mà không ai kịp nói không.
//!
//! Gỡ plugin giết sạch job nền. Một tiến trình sống lâu hơn thứ sinh ra nó là một tiến
//! trình không ai còn nhớ để dọn.

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

/// Đẩy mọi lời gọi `bash` qua người dùng.
///
/// Là một middleware chứ không phải một canh gác, vì canh gác cố ý **đơn điệu**: chúng
/// chỉ từ chối hoặc bỏ qua, không mở được đường hỏi. Đó là thiết kế đúng — nếu canh gác
/// trả về `Ask` được thì thứ tự đăng ký sẽ biến một lệnh từ chối thành một câu hỏi, và
/// một câu hỏi thì trả lời "có" được.
///
/// Là một plugin riêng chứ không phải một trường trong `ToolMeta`, vì chính sách phải gỡ
/// ra được: một bản chạy không giao diện với sandbox thật sẽ thay nó bằng cái khác, và
/// lúc đó không ai phải sửa `bash`.
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
            // Uỷ quyền trước rồi mới hỏi: nếu một tầng dưới đã từ chối thì không có gì
            // để hỏi, và hỏi một câu mà câu trả lời không đổi được gì là làm người dùng
            // quen với việc bấm cho qua.
            match next.run(req).await {
                // Câu hỏi phải nói đúng mức rủi ro thật. Người dùng đọc đúng dòng này
                // trước khi bấm "cho phép", nên một câu chung chung ở đây là một câu
                // khiến họ quen với việc bấm cho qua.
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
        // `workspace-write` là mặc định: một coding agent phải sửa được repo, và mọi
        // thứ ngoài repo thì không. Chế độ chặt hơn hay lỏng hơn là quyết định của phiên.
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
