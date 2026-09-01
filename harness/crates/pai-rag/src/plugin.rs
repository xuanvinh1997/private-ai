//! Cắm thư viện tài liệu vào cây.
//!
//! Một plugin, một hoặc hai provider, ba tool. Gỡ nó ra là mất `docs.search`, `docs.read`
//! và `docs.list`, và không mất gì khác: không tool nào của crate khác gọi vào thư viện,
//! nên không có luật nào ở lại canh giữ những tool không còn ở đó.
//!
//! Plugin này thuộc **tầng dự án** — nó cần một đường dẫn, nên đổi dự án là tháo nó ra và
//! cắm lại với thư mục mới. Xem `docs/ARCHITECTURE.md`, mục "Dự án, và hai tầng plugin".

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_tools::Tools;

use crate::embed::{Embedder, Embeddings};
use crate::library::{DocLibrary, Docs, Library};
use crate::tools::list::DocsList;
use crate::tools::read::DocsRead;
use crate::tools::search::DocsSearch;

pub struct RagPlugin {
    /// Thư mục của dự án tài liệu: cơ sở dữ liệu và thư mục `files/` nằm trong đó.
    dir: PathBuf,
    /// `None` là hợp lệ và là trường hợp thường gặp lúc mới cài: thư viện chạy bằng FTS5
    /// cho tới khi người dùng chọn được một mô hình nhúng.
    embedder: Option<Arc<dyn Embedder>>,
}

impl RagPlugin {
    pub fn new(dir: PathBuf, embedder: Option<Arc<dyn Embedder>>) -> RagPlugin {
        RagPlugin { dir, embedder }
    }
}

#[async_trait]
impl Plugin for RagPlugin {
    fn name(&self) -> &'static str {
        "rag"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let library = Arc::new(Library::open(&self.dir, self.embedder.clone())?);

        // Gộp WAL lúc tháo. Không có bước này thì thư mục dự án ở lại với một tệp `-wal`
        // mà lần mở sau phải phát lại — vô hại, nhưng nó cũng có nghĩa là sao lưu thư mục
        // dự án ngay sau khi đóng ứng dụng sẽ chép về một cơ sở dữ liệu chưa gộp.
        let closing = library.clone();
        ctx.effects().defer("rag/checkpoint", move || {
            if let Err(err) = closing.checkpoint() {
                tracing::debug!(%err, "không gộp được WAL của thư viện tài liệu");
            }
        });

        let docs: Arc<dyn DocLibrary> = library;
        ctx.keep(ctx.provide::<Docs>(docs.clone())?);

        // Bộ nhúng cũng lên seam: những thứ khác — một trang cấu hình chẳng hạn — cần hỏi
        // `health()` mà không nên phải đi qua thư viện để hỏi.
        if let Some(embedder) = self.embedder.clone() {
            ctx.keep(ctx.provide::<Embeddings>(embedder)?);
        }

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(DocsSearch::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsRead::new(docs.clone()))));
        ctx.keep(tools.register(Arc::new(DocsList::new(docs))));
        Ok(())
    }
}
