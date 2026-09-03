//! pai-llm — từ vựng message/stream, và seam adapter mô hình.
//!
//! Bản Python không có tầng này: nó mượn thẳng `BaseChatModel` của LangChain, nên hình
//! dạng chunk và wire format tool-calling nằm hết trong thư viện. Ở đây phải tự viết, và
//! crate được chia theo đúng ranh giới ấy:
//!
//! - [`message`] và [`stream`] — **hợp đồng**, trung lập với provider. Đây là thứ mà
//!   `pai-agent` và `pai-session` phụ thuộc vào.
//! - [`assembler`] — gấp luồng chunk thành message. Chỗ dễ sai nhất của cả crate: tham
//!   số tool đến từng mảnh dưới dạng chuỗi JSON bị cắt vụn.
//! - [`wire`] — byte của socket thành khung giao thức. Tách ra để bộ test chạy được mà
//!   không cần mạng.
//! - [`ollama`], [`lmstudio`] và [`openai`] — ba bản cài đặt. Chúng sở hữu phần dây; hợp
//!   đồng thì không. `lmstudio` mượn nguyên dây hội thoại của `openai` và chỉ thêm nửa
//!   kho mô hình, vì đó đúng là chỗ LM Studio khác một máy chủ OpenAI-compatible bất kỳ.
//! - [`seam`] — hai `ServiceKey`: nói chuyện với mô hình, và vòng đời mô hình cục bộ.
//!
//! Không dùng `async-openai`. Nửa quan trọng nhất của một ứng dụng chạy mô hình tại chỗ
//! là admin API của Ollama — kéo về, liệt kê, xem, nhả, xoá — mà crate ấy không có, và
//! `/api/show` lại đúng là **nguồn có thẩm quyền** cho năng lực mô hình. `reqwest` cộng
//! một bộ phân tích SSE sáu chục dòng đổi lại quyền kiểm soát trọn vẹn chỗ hay hỏng.

pub mod assembler;
pub mod capabilities;
pub mod error;
pub mod lmstudio;
pub mod message;
pub mod model;
pub mod ollama;
pub mod openai;
pub mod registry;
pub mod seam;
pub mod stream;
pub mod wire;

pub use assembler::BlockAssembler;
pub use capabilities::{Capabilities, CapabilitySource};
pub use error::{LlmError, LlmErrorCode};
pub use message::{ChatRequest, ContentBlock, Message, ToolCall, ToolCallId, ToolSchema};
pub use model::{ModelDetails, ModelInfo, ModelState, PullProgress, RunningModel};
pub use lmstudio::{LmStudioAdapter, LmStudioAdmin};
pub use ollama::{OllamaAdapter, OllamaAdmin};
pub use openai::{OpenAiAdapter, openai_base_url};
pub use registry::{
    AdapterRegistry, ProviderConfig, ProviderKind, ProviderSignature, active_config,
};
pub use seam::{Llm, LlmAdapter, ModelAdmin, Models};
pub use stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};
