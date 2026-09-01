//! Plugin.
//!
//! Một plugin không có đặc quyền nào: nó nhận một `Context` và đóng góp service,
//! listener, hoặc cả hai. Mọi đăng ký nó tạo ra thuộc về scope hiệu ứng của nó, nên gỡ
//! tải chỉ là gọi disposer — không có bảng đăng ký nào phải dọn bằng tay.

use async_trait::async_trait;

use crate::context::Context;

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Tên cho log và `--dump-config`.
    fn name(&self) -> &'static str;

    /// Cắm vào cây. `ctx` đã là ngữ cảnh riêng của plugin này.
    async fn apply(&self, ctx: &Context) -> anyhow::Result<()>;
}
