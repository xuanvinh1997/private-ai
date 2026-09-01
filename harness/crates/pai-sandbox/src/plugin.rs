//! Cắm vòng giam vào cây.
//!
//! Plugin không chọn chính sách — nó chỉ cắm một provider. Chính sách (`read-only` hay
//! `workspace-write`) thuộc về phiên, vì cùng một máy có thể chạy một agent chỉ-đọc bên
//! cạnh một agent được sửa repo, và cả hai dùng chung đúng một vòng giam.
//!
//! Không có provider nào là một trạng thái **hợp lệ**, không phải một lỗi cấu hình: bản
//! chạy trong test và bản chạy trên hệ điều hành chưa hỗ trợ đều rơi vào đó. Người tiêu
//! thụ phải xử lý được trường hợp seam trống, và `for_this_machine` luôn trả về một
//! provider có lý do chứ không trả về "không có gì" — "không ai trả lời" và "trả lời là
//! không giam được" là hai câu khác nhau đối với hộp thoại duyệt.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::seam::{Sandbox, SandboxProvider, for_this_machine};

pub struct SandboxPlugin {
    provider: Arc<dyn SandboxProvider>,
}

impl Default for SandboxPlugin {
    fn default() -> SandboxPlugin {
        SandboxPlugin {
            provider: for_this_machine(),
        }
    }
}

impl SandboxPlugin {
    /// Provider cho máy đang chạy.
    pub fn new() -> SandboxPlugin {
        SandboxPlugin::default()
    }

    /// Provider chỉ định sẵn. Dành cho test và cho bản chạy từ xa, nơi vòng giam nằm ở
    /// đầu bên kia chứ không nằm trên máy này.
    pub fn with_provider(provider: Arc<dyn SandboxProvider>) -> SandboxPlugin {
        SandboxPlugin { provider }
    }
}

#[async_trait]
impl Plugin for SandboxPlugin {
    fn name(&self) -> &'static str {
        "sandbox"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let enforcement = self.provider.enforcement();
        match enforcement.reason() {
            // Ghi ở mức `warn` chứ không `info`: một bản cài đặt chạy không có vòng giam
            // mà không ai nhận ra là đúng cái tình huống mà `Enforcement` sinh ra để tránh.
            Some(reason) => {
                tracing::warn!(
                    mode = enforcement.label(),
                    "giam tiến trình hạn chế: {reason}"
                )
            }
            None => tracing::info!(mode = enforcement.label(), "giam tiến trình đầy đủ"),
        }
        ctx.keep(ctx.provide::<Sandbox>(self.provider.clone())?);
        Ok(())
    }
}
