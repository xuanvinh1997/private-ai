//! Lõi plugin: mọi thứ khác trong harness là một plugin cắm vào đây.
//!
//! Bốn ý, mượn từ Cordis nhưng viết lại cho hệ kiểu của Rust:
//!
//! - **Seam** — một khả năng được đánh địa chỉ bằng marker type, không phải bằng bản
//!   cài đặt. Đổi provider không đụng tới consumer. Xem [`service::ServiceKey`].
//! - **Phụ thuộc là nhu cầu, không phải trình tự** — plugin `wait_for` service nó cần,
//!   nên thứ tự khởi động tự sắp xếp. Xem [`context::Context::wait_for`].
//! - **Sự kiện có kiểu** — quan sát, chọn-người-đầu-tiên, và middleware bao quanh.
//!   Xem [`event`].
//! - **Đăng ký là hiệu ứng gỡ lại được** — guard RAII cho mặc định, scope tường minh
//!   khi việc dọn cần `await`. Xem [`effect`].

pub mod config;
pub mod context;
pub mod effect;
pub mod event;
pub mod plugin;
pub mod scope;
pub mod service;

pub use config::{Composed, ConfigError, Layer, Patch, PluginCatalog, Row, compose};
pub use context::{Context, ProvideError};
pub use effect::{EffectScope, Guard};
pub use event::{First, Middleware, Next, Notify, Waterfall};
pub use plugin::Plugin;
pub use scope::ScopeKey;
pub use service::{Realm, ServiceKey};
