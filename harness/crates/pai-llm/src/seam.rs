//! Seam của tầng mô hình: hai khả năng thay thế được, khai bằng marker type.
//!
//! Bản Python không có seam — nó gọi thẳng `ChatOllama`/`ChatOpenAI` (`llm/router.py`),
//! nên "đổi provider" và "đổi thư viện" là cùng một việc. Ở đây tách đôi:
//!
//! - [`Llm`] — nói chuyện với mô hình. Mọi provider đều có.
//! - [`Models`] — vòng đời mô hình cục bộ: kéo về, xem, nhả, xoá. **Chỉ Ollama có.**
//!   Đây đúng là nửa mà LangChain không biết tới và bản Python phải viết tay bằng httpx
//!   (`llm/admin.py`, 273 dòng). Tách nó thành seam riêng để một provider từ xa trả lời
//!   "không áp dụng" ở tầng kiểu, chứ không phải bằng một ngoại lệ lúc chạy.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_core::ServiceKey;

use crate::capabilities::Capabilities;
use crate::error::LlmError;
use crate::message::ChatRequest;
use crate::model::{ModelDetails, ModelInfo, PullProgress, RunningModel};
use crate::stream::StreamChunk;

/// Nói chuyện với một mô hình.
///
/// **`stream` không phải `async fn`, và đó là cố ý.** `#[async_trait]` biến một `async fn`
/// thành hàm trả `Pin<Box<dyn Future>>`; nếu `stream` cũng đi qua đó thì cái ta thực sự
/// nhận được là `Box<dyn Future<Output = Box<dyn Stream>>>` — hai lần cấp phát trên heap
/// cho một giá trị mà bản thân nó đã là con trỏ béo, cộng một `.await` bắt buộc trước khi
/// người gọi kịp thấy chunk đầu tiên. Trả `BoxStream` trực tiếp thì việc gửi request nằm
/// *bên trong* stream, nên huỷ trước khi kết nối xong cũng chỉ là thả cái stream đi.
///
/// Các phương thức còn lại là `async fn` bình thường: chúng trả một giá trị, không trả
/// một luồng, nên `#[async_trait]` không mất mát gì. `#[async_trait]` trên trait chỉ viết
/// lại các `async fn`; `stream` đi qua nguyên vẹn.
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// Id của provider mà adapter này phục vụ. Có mặt trong log và thông báo lỗi.
    fn id(&self) -> &str;

    /// Luồng chunk cho một request.
    ///
    /// Bất biến: luồng kết thúc bằng đúng một [`StreamChunk::Finish`], hoặc bằng một
    /// `Err`. Huỷ = thả stream.
    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>>;

    /// Mô hình này làm được gì. Bản cài đặt phải hỏi máy chủ trước, đoán theo tên sau.
    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError>;

    /// Máy chủ có trả lời không. Trả `bool` chứ không `Result` vì mọi cách hỏng đều cho
    /// cùng một câu trả lời, và người gọi chỉ dùng nó để bật/tắt một chấm tròn.
    async fn health(&self) -> bool {
        true
    }

    /// Nửa vòng đời, nếu provider này có. `None` nghĩa là chỉ đọc — mô hình nằm ở nơi khác.
    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        None
    }
}

/// Vòng đời mô hình cục bộ.
#[async_trait]
pub trait ModelAdmin: Send + Sync {
    /// Kho mô hình, kèm trạng thái nạp và năng lực.
    async fn list(&self) -> Result<Vec<ModelInfo>, LlmError>;

    /// Mô hình đang thường trú trong VRAM.
    async fn running(&self) -> Result<Vec<RunningModel>, LlmError>;

    /// Metadata có thẩm quyền của một mô hình.
    async fn show(&self, model: &str) -> Result<ModelDetails, LlmError>;

    /// Kéo một mô hình về, phát tiến trình.
    ///
    /// Trả `BoxStream` vì lý do y hệt `LlmAdapter::stream`, cộng một lý do nữa: một bản
    /// tải nhiều gigabyte không có thời hạn hợp lý nào, nên cách duy nhất để dừng nó là
    /// **thả cái stream đi**, và việc đó đóng kết nối, và việc đó huỷ bản tải phía máy chủ.
    fn pull(&self, model: &str) -> BoxStream<'_, Result<PullProgress, LlmError>>;

    /// Nhả một mô hình khỏi VRAM.
    async fn unload(&self, model: &str) -> Result<(), LlmError>;

    /// Xoá khỏi đĩa.
    async fn delete(&self, model: &str) -> Result<(), LlmError>;
}

/// Seam: nói chuyện với mô hình.
pub enum Llm {}

impl ServiceKey for Llm {
    type Api = dyn LlmAdapter;
    const NAME: &'static str = "llm";
}

/// Seam: vòng đời mô hình cục bộ.
pub enum Models {}

impl ServiceKey for Models {
    type Api = dyn ModelAdmin;
    const NAME: &'static str = "llm.models";
}
