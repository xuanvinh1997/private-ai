//! Cắm sổ đăng ký vào cây.
//!
//! Sổ phải là một seam chứ không phải một biến toàn cục, vì mỗi phiên có thể có bộ tool
//! khác nhau và vì bài kiểm chứng phải dựng được một cây riêng. Kho tràn đi kèm ở đây:
//! không có nó thì output dài bị gửi nguyên vẹn cho mô hình, và một lần `grep` rộng tay
//! đẩy hết ngữ cảnh còn lại ra ngoài cửa sổ.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::builtin::todo::TodoWrite;
use crate::registry::ToolRegistry;
use crate::seam::{Spill, Tools};
use crate::spill::{MemorySpillStore, SpillStore};

#[derive(Default)]
pub struct ToolsPlugin;

#[async_trait]
impl Plugin for ToolsPlugin {
    fn name(&self) -> &'static str {
        "tools"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let registry = ToolRegistry::new(ctx);
        // `todo_write` ở đây chứ không ở một plugin riêng: nó không đụng đĩa, không đụng
        // mạng, và không có gì để tắt đi. Một plugin cho một tool không có phụ thuộc nào
        // chỉ là một tệp nữa phải đọc.
        ctx.keep(registry.register(Arc::new(TodoWrite::new())));
        ctx.keep(ctx.provide::<Tools>(registry)?);
        let spill: Arc<dyn SpillStore> = Arc::new(MemorySpillStore::default());
        ctx.keep(ctx.provide::<Spill>(spill)?);
        Ok(())
    }
}
