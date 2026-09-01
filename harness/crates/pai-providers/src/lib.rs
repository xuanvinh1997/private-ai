//! pai-providers — cấu hình nhà cung cấp mô hình, và đổi giữa chúng lúc đang chạy.
//!
//! `pai-llm` biết dựng adapter cho một [`pai_llm::ProviderConfig`] nhưng cố ý không biết
//! cấu hình ấy từ đâu ra. Crate này là nửa còn lại:
//!
//! - [`store`] — hàng trên đĩa, kèm khoá API. Tệp riêng, quyền `0600`. Một danh sách
//!   provider, **hai vai**: xem [`store::Role`].
//! - [`embed`] — bộ nhúng dựng từ provider đang giữ vai nhúng.
//! - [`presets`] — danh mục dựng sẵn, để người dùng không phải gõ đúng một URL.
//! - [`probe`] — thử một cấu hình trước khi lưu, và nói ra **phải làm gì tiếp theo**.
//! - [`runtime`] — một đường duy nhất để đổi provider: đĩa, cache adapter, và `Driver`
//!   luôn đi cùng nhau.
//!
//! Điều bất biến quan trọng nhất không nằm ở đây mà ở `pai_agent::Driver`: cú đổi có hiệu
//! lực từ **lượt sau**, không phải bước sau. Xem bình luận trong `Driver::drive`.

pub mod embed;
pub mod error;
pub mod presets;
pub mod probe;
pub mod runtime;
pub mod seam;
pub mod store;

pub use embed::{
    DEFAULT_EMBEDDING_MODEL_OLLAMA, DEFAULT_EMBEDDING_MODEL_OPENAI, default_embedding_model,
    embedder_for, embedding_reason,
};
pub use error::{ProviderError, Result};
pub use presets::{PRESETS, Preset};
pub use probe::{EmbeddingProbeResult, ProbeModel, ProbeResult, probe, probe_embedding};
pub use runtime::ProviderRuntime;
pub use seam::Providers;
pub use store::{DB_FILE, ProviderInput, ProviderStore, Role, SqliteProviderStore, StoredProvider};
