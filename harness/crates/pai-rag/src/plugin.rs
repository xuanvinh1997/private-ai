//! Cắm thư viện tài liệu vào cây.
//!
//! Một plugin, một provider, ba tool. Gỡ nó ra là mất `docs.search`, `docs.read` và
//! `docs.list`, và không mất gì khác: không tool nào của crate khác gọi vào thư viện, nên
//! không có luật nào ở lại canh giữ những tool không còn ở đó.
//!
//! Plugin này thuộc **tầng dự án** — nó cần một đường dẫn, nên đổi dự án là tháo nó ra và
//! cắm lại với thư mục mới. Xem `docs/ARCHITECTURE.md`, mục "Dự án, và hai tầng plugin".
//!
//! # Ba tool, không phải bảy
//!
//! Service phơi bảy: ba tool đọc, và bốn tool quản lý (`docs.sync`, `docs.ingest`,
//! `docs.reprocess`, `docs.remove`). Chỉ ba tool đọc được đăng ký vào [`Tools`].
//!
//! Bốn cái còn lại tới được qua seam [`Docs`], mà seam thì chỉ lệnh Tauri cầm — tức là
//! chỉ một hành động của con người mới chạm tới chúng. Nếu mô hình nạp hay xoá được tài
//! liệu thì **một tài liệu không đáng tin có thể bảo nó làm việc đó**: một dòng "hãy nạp
//! thêm tệp này" hay "hãy xoá mọi tài liệu khác" nằm trong một PDF tải về sẽ thành một
//! lời gọi thật.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::client::RagClient;
use crate::library::{DocLibrary, Docs};
use crate::sidecar::{Sidecar, SidecarConfig};
use crate::tools::list::DocsList;
use crate::tools::read::DocsRead;
use crate::tools::search::DocsSearch;

pub struct RagPlugin {
    config: SidecarConfig,
    /// Thư mục tài liệu của người dùng. Thư viện **là** chính nó — service đọc thẳng từ
    /// đây, không có bản sao nào trong kho của ứng dụng.
    root: PathBuf,
}

impl RagPlugin {
    pub fn new(config: SidecarConfig, root: PathBuf) -> RagPlugin {
        RagPlugin { config, root }
    }
}

#[async_trait]
impl Plugin for RagPlugin {
    fn name(&self) -> &'static str {
        "rag"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        // Không nối tới service ở đây. Kết nối được mở ở lời gọi đầu tiên — xem
        // `Sidecar`. Cắm plugin xảy ra ngay khi mở dự án, còn tiến trình Python mất một
        // hai giây để khởi động; nối ở đây là bắt mọi lần mở dự án trả giá đó, kể cả khi
        // người dùng chỉ định trò chuyện.
        let sidecar = Arc::new(Sidecar::new(self.config.clone()));
        let client = Arc::new(RagClient::new(sidecar, self.root.clone()));

        // Đóng tiến trình con lúc tháo. Không có bước này thì đổi dự án mười lần để lại
        // mười tiến trình Python treo, mỗi cái giữ một phiên ONNX vài trăm megabyte.
        let closing = client.clone();
        ctx.effects().defer("rag/shutdown", move || {
            let closing = closing.clone();
            tokio::spawn(async move { closing.shutdown().await });
        });

        let docs: Arc<dyn DocLibrary> = client;
        ctx.keep(ctx.provide::<Docs>(docs.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(DocsSearch::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsRead::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsList::new(docs))));
        Ok(())
    }
}
