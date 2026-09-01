//! Cắm LSP vào cây — và **không** cắm nó khi không có gì để cắm.
//!
//! Đây là toàn bộ chỗ khác biệt giữa crate này và mọi crate tool khác trong harness.
//! `read`, `grep`, `bash` luôn dùng được vì hệ tệp và shell luôn có mặt. Language server
//! thì không: `rust-analyzer`, `typescript-language-server`, `pyright` đều là thứ người
//! dùng phải tự cài, và trên một máy vừa cài ứng dụng thì thường không có cái nào.
//!
//! Luật ở đây, và nó là luật cứng:
//!
//! > **Không dò được server nào thì không tool nào được đăng ký**, chứ không phải đăng ký
//! > rồi hỏng lúc gọi.
//!
//! Một tool có trong danh sách mà lần nào gọi cũng lỗi không chỉ vô dụng — nó **dạy mô
//! hình bỏ qua danh sách**. Sau vài lần `lsp` trả về "không có provider", mô hình học
//! rằng danh sách tool là một lời gợi ý chứ không phải một hợp đồng, và cái nó học được
//! áp cho cả `read` lẫn `bash`. Cái giá của luật này là mô tả tool đổi theo máy; cái giá
//! của việc không có nó là một mô hình hoài nghi mọi thứ ta nói với nó.
//!
//! Và việc dò diễn ra **một lần**, ở đây, chứ không phải mỗi lần gọi — xem
//! [`crate::launch::locate`].

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_tools::Tools;

use crate::config::{LanguageConfig, Limits, defaults};
use crate::launch::{ChildLaunch, Launch, locate};
use crate::pool::{Entry, StdioServers};
use crate::seam::{LanguageServers, Lsp};
use crate::tool::LspTool;

pub struct LspPlugin {
    roots: FileRoots,
    workspace: PathBuf,
    languages: Vec<LanguageConfig>,
    limits: Limits,
}

impl LspPlugin {
    /// `roots` và `protected` nên là **cùng bộ** đã cấp cho `FsPlugin`: một tool đọc được
    /// mã ở chỗ `read` không với tới là một đường vòng quanh chính ranh giới đó.
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
        workspace: PathBuf,
    ) -> LspPlugin {
        LspPlugin {
            roots: FileRoots::new(roots, protected),
            workspace,
            languages: defaults(),
            limits: Limits::default(),
        }
    }

    /// Thay cả bảng ngôn ngữ. Thay cả khối chứ không trộn, cùng luật với cấu hình theo lớp.
    pub fn with_languages(mut self, languages: Vec<LanguageConfig>) -> LspPlugin {
        self.languages = languages;
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> LspPlugin {
        self.limits = limits;
        self
    }
}

#[async_trait]
impl Plugin for LspPlugin {
    fn name(&self) -> &'static str {
        "lsp"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let mut entries: Vec<Entry> = Vec::new();
        for config in self.languages.iter().filter(|row| row.enabled) {
            let Some(command) = locate(&config.command) else {
                tracing::debug!(
                    language = %config.id, command = %config.command,
                    "không có trên máy này; bỏ qua"
                );
                continue;
            };
            let launcher: Arc<dyn Launch> = Arc::new(ChildLaunch::new(
                config.id.clone(),
                command,
                config.args.clone(),
                self.workspace.clone(),
            ));
            entries.push(Entry {
                id: config.id.clone(),
                extensions: config.extensions.clone(),
                launcher,
                options: config.initialization_options.clone(),
            });
        }

        if entries.is_empty() {
            // Không provider, không tool, và **không lỗi**: một máy chưa cài language
            // server nào là một máy bình thường, không phải một cấu hình hỏng.
            tracing::info!("không dò được language server nào; tool `lsp` không được đăng ký");
            return Ok(());
        }
        tracing::info!(
            languages = ?entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            "đã dò được language server"
        );

        let servers = Arc::new(StdioServers::new(
            self.workspace.clone(),
            self.roots.clone(),
            entries,
            self.limits,
        ));
        let seam: Arc<dyn LanguageServers> = servers.clone();
        ctx.keep(ctx.provide::<Lsp>(seam.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(LspTool::new(seam))));

        // `shutdown`/`exit` cho mọi server đang chạy. Dọn bất đồng bộ, nên nó phải là một
        // `defer_async` chứ không phải một `Drop`: giết ống mà không nói `exit` để lại
        // những tiến trình con sống tới hết phiên đăng nhập của người dùng.
        ctx.effects()
            .defer_async("lsp/servers", move || async move {
                servers.shutdown().await;
            });
        Ok(())
    }
}
