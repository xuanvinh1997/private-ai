//! pai-providers — cấu hình nhà cung cấp mô hình, và đổi giữa chúng lúc đang chạy.
//!
//! `pai-llm` biết dựng adapter cho một [`pai_llm::ProviderConfig`] nhưng cố ý không biết
//! cấu hình ấy từ đâu ra. Crate này là nửa còn lại:
//!
//! - [`store`] — hàng trên đĩa, kèm khoá API. Tệp riêng, quyền `0600`.
//! - [`presets`] — danh mục dựng sẵn, để người dùng không phải gõ đúng một URL.
//! - [`probe`] — thử một cấu hình trước khi lưu, và nói ra **phải làm gì tiếp theo**.
//! - [`runtime`] — một đường duy nhất để đổi provider: đĩa, cache adapter, và `Driver`
//!   luôn đi cùng nhau.
//!
//! Điều bất biến quan trọng nhất không nằm ở đây mà ở `pai_agent::Driver`: cú đổi có hiệu
//! lực từ **lượt sau**, không phải bước sau. Xem bình luận trong `Driver::drive`.

pub mod error;
pub mod presets;
pub mod probe;
pub mod runtime;
pub mod seam;
pub mod store;

pub use error::{ProviderError, Result};
pub use presets::{PRESETS, Preset};
pub use probe::{ProbeModel, ProbeResult, probe};
pub use runtime::ProviderRuntime;
pub use seam::Providers;
pub use store::{DB_FILE, ProviderInput, ProviderStore, SqliteProviderStore, StoredProvider};
