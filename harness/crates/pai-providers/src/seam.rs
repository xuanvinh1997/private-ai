//! Seam của crate này.
//!
//! Một khoá duy nhất, và `Api` là kiểu cụ thể chứ không phải trait object: cái mà phần
//! còn lại của cây cần không phải "một cách đổi provider nào đó" mà là **đúng cái đường
//! duy nhất** ở [`ProviderRuntime`]. Một trait ở đây chỉ mở ra một đường thứ hai để quên
//! cập nhật một thứ — chính cái mà runtime sinh ra để chặn.

use pai_core::ServiceKey;

use crate::runtime::ProviderRuntime;

pub enum Providers {}

impl ServiceKey for Providers {
    type Api = ProviderRuntime;
    const NAME: &'static str = "providers";
}
