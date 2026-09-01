//! Cắm chỉ mục vào cây.
//!
//! Một plugin, một provider, hai tool. Gỡ nó ra là mất `symbol_search` và `outline`, và
//! không mất gì khác: không tool nào của crate khác gọi vào chỉ mục, nên không có luật
//! nào ở lại canh giữ những tool không còn ở đó.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_tools::Tools;

use crate::index::{CodeIndex, Index, SymbolIndex};
use crate::tools::outline::Outline;
use crate::tools::symbol_search::SymbolSearch;

pub struct IndexPlugin {
    roots: FileRoots,
    /// Thư mục chứa tệp chỉ mục, không phải chính tệp đó — xem [`db_name`].
    dir: PathBuf,
}

impl IndexPlugin {
    /// `roots` và `protected` nên là **cùng bộ** đã cấp cho `FsPlugin`: một chỉ mục nhìn
    /// rộng hơn hệ tệp là một đường vòng quanh chính cái ranh giới đó.
    pub fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        protected: impl IntoIterator<Item = PathBuf>,
        dir: PathBuf,
    ) -> IndexPlugin {
        IndexPlugin {
            roots: FileRoots::new(roots, protected),
            dir,
        }
    }
}

/// Tên tệp chỉ mục cho một thư mục làm việc.
///
/// Tên tệp được **suy ra** chứ không nhận từ ngoài, và đó là một quyết định chứ không
/// phải một tiện lợi: một đường dẫn cố định do người gọi truyền vào thì hai workspace
/// dùng chung một chỉ mục, và triệu chứng của việc đó là `symbol_search` trả về những
/// hàm của một dự án khác — một lỗi trông y hệt một chỉ mục chỉ đơn giản là sai.
///
/// Băm là FNV-1a: nó không cần chống va chạm có chủ ý, nó chỉ cần phân biệt hai đường dẫn
/// trên cùng một máy, và một dependency mật mã cho việc đó là trả giá cho thứ không dùng.
fn db_name(root: &std::path::Path) -> String {
    let text = root.display().to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let label: String = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into())
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    let label = if label.is_empty() {
        "root".to_string()
    } else {
        label
    };
    format!("{label}-{hash:016x}.sqlite")
}

#[async_trait]
impl Plugin for IndexPlugin {
    fn name(&self) -> &'static str {
        "index"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let root = self
            .roots
            .roots()
            .first()
            .ok_or_else(|| anyhow::anyhow!("chỉ mục cần ít nhất một thư mục được cấp quyền"))?;
        let db = self.dir.join(db_name(root));
        let index: Arc<dyn SymbolIndex> = Arc::new(CodeIndex::open(self.roots.clone(), &db)?);
        ctx.keep(ctx.provide::<Index>(index.clone())?);

        let tools = ctx.require::<Tools>()?;
        ctx.keep(tools.register(Arc::new(SymbolSearch::new(index.clone()))));
        ctx.keep(tools.register(Arc::new(Outline::new(index, self.roots.clone()))));
        Ok(())
    }
}
