//! Một con trỏ tới **provider đang hoạt động**, chia sẻ cho mọi chỗ cần nói chuyện với mô
//! hình.
//!
//! Không có tệp này thì việc đổi provider chỉ đổi được đúng một nửa. `Driver` giữ một
//! `ArcSwap` nên nó theo kịp, nhưng ba chỗ khác thì không: `subagent` nhận adapter lúc cắm
//! plugin, `Rebuild` giữ một bản để dựng lại tầng dự án, và phần quản trị mô hình giữ một
//! bản nữa để liệt kê. Ba bản sao ấy được lấy lúc khởi động và không ai cập nhật chúng —
//! nên sau khi người dùng đổi từ Ollama sang một provider từ xa, agent con vẫn lặng lẽ gọi
//! máy chủ cũ, và màn hình mô hình vẫn liệt kê kho cũ. Đó là kiểu hỏng tệ nhất: không có
//! thông báo lỗi nào, chỉ có câu trả lời đến từ chỗ người dùng nghĩ là đã tắt.
//!
//! Cách chữa là **không phát bản sao nào cả**. Mọi chỗ nhận cùng một `ActiveLlm`, và đổi
//! provider là đổi cái nó trỏ tới.

use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_llm::{Capabilities, ChatRequest, LlmAdapter, LlmError, ModelAdmin, StreamChunk};

pub struct ActiveLlm {
    // Hai lớp `Arc` là bắt buộc: `arc-swap` chỉ nhận `Arc<T>` với `T: Sized`, còn
    // `dyn LlmAdapter` thì không. Đây cũng đúng là cách `Driver` phải làm.
    inner: ArcSwap<Arc<dyn LlmAdapter>>,
}

impl ActiveLlm {
    pub fn new(initial: Arc<dyn LlmAdapter>) -> ActiveLlm {
        ActiveLlm {
            inner: ArcSwap::from_pointee(initial),
        }
    }

    pub fn set(&self, next: Arc<dyn LlmAdapter>) {
        tracing::info!(provider = next.id(), "đổi provider đang hoạt động");
        self.inner.store(Arc::new(next));
    }

    pub fn current(&self) -> Arc<dyn LlmAdapter> {
        Arc::clone(&self.inner.load())
    }
}

#[async_trait]
impl LlmAdapter for ActiveLlm {
    /// Hằng số, không phải id của provider bên dưới.
    ///
    /// `id` trả `&str` mượn từ `self`, mà cái đang hoạt động lại nằm sau một `ArcSwap` —
    /// không có cách nào trả về tham chiếu vào một giá trị có thể bị thay ngay sau đó. Id
    /// thật của provider vẫn đi vào log ở [`ActiveLlm::set`] và trong `pai-providers`, nên
    /// thông tin không mất, chỉ đổi chỗ.
    fn id(&self) -> &str {
        "đang-hoạt-động"
    }

    /// Bắc cầu qua một kênh thay vì trả thẳng luồng của adapter bên dưới.
    ///
    /// Không phải vì thích, mà vì hệ kiểu không cho cách khác: `stream` trả
    /// `BoxStream<'_>` mượn từ `&self`, còn adapter đang hoạt động nằm sau một `ArcSwap`
    /// nên nó là một giá trị **sở hữu** lấy ra lúc gọi. Một luồng vừa mượn từ một `Arc`
    /// cục bộ vừa mang cái `Arc` ấy theo là một cấu trúc tự tham chiếu; viết được, nhưng
    /// chỉ bằng `unsafe` hoặc bằng một crate nữa cho đúng một chỗ này.
    ///
    /// Cái giá là một lần chuyển tay cho mỗi chunk. Nó nhỏ so với chặng mạng đứng ngay
    /// trước nó, và nhỏ hơn nữa so với bộ gộp token 16 ms đứng ngay sau. Huỷ vẫn đúng:
    /// thả luồng là thả `Receiver`, `send` hỏng, và tác vụ bơm thoát — không có tiến trình
    /// nào bị bỏ lại.
    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        // Chốt adapter **trước** khi mở luồng: một lượt phải đi trọn vẹn tới cùng một máy
        // chủ. Đổi provider giữa chừng là gửi nửa hội thoại đi một nơi và nửa kia đi nơi
        // khác — cùng bất biến mà `Driver` giữ ở tầng trên.
        let adapter = self.current();
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let mut inner = adapter.stream(req);
            while let Some(chunk) = futures::StreamExt::next(&mut inner).await {
                if tx.send(chunk).await.is_err() {
                    // Người nhận đã đi. Thả `inner` ở đây chính là cú huỷ.
                    break;
                }
            }
        });
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    async fn capabilities(&self, model: &str) -> Result<Capabilities, LlmError> {
        self.current().capabilities(model).await
    }

    async fn health(&self) -> bool {
        self.current().health().await
    }

    fn admin(&self) -> Option<Arc<dyn ModelAdmin>> {
        self.current().admin()
    }
}

/// Bộ nhúng của **provider đang hoạt động**, cùng lý do như [`ActiveLlm`].
///
/// Dễ hơn hẳn phía hội thoại: [`Embedder::embed`] trả một giá trị chứ không trả một luồng,
/// nên không có ràng buộc vòng đời nào và bản uỷ quyền chỉ là ba dòng.
///
/// `None` là trạng thái hợp lệ và là trạng thái thường gặp lúc mới cài: chưa có provider
/// nào cấu hình xong thì thư viện tài liệu chạy bằng FTS5, và `LibraryStats::reason` nói
/// ra điều đó. Nó không phải lỗi, nên nó không được ném lỗi.
pub struct ActiveEmbedder {
    inner: arc_swap::ArcSwapOption<Arc<dyn pai_rag::Embedder>>,
}

impl ActiveEmbedder {
    pub fn empty() -> ActiveEmbedder {
        ActiveEmbedder {
            inner: arc_swap::ArcSwapOption::empty(),
        }
    }

    pub fn set(&self, next: Option<Arc<dyn pai_rag::Embedder>>) {
        self.inner.store(next.map(Arc::new));
    }

    pub fn current(&self) -> Option<Arc<dyn pai_rag::Embedder>> {
        self.inner.load_full().map(|item| Arc::clone(item.as_ref()))
    }
}

#[async_trait]
impl pai_rag::Embedder for ActiveEmbedder {
    fn id(&self) -> &str {
        // Cùng ràng buộc như [`ActiveLlm::id`]: không trả được tham chiếu vào một giá trị
        // thay được. Chuỗi này đi thẳng vào `LibraryStats::embedder` trên màn hình, nên nó
        // phải đọc được — và câu trung thực nhất là nói rằng nó theo provider đang chọn.
        "theo nhà cung cấp đang chọn"
    }

    fn dim(&self) -> Option<usize> {
        self.current().and_then(|item| item.dim())
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, pai_rag::RagError> {
        match self.current() {
            Some(inner) => inner.embed(texts).await,
            // Không có bộ nhúng là chuyện bình thường, nhưng một lời gọi `embed` khi không
            // có thì vẫn phải hỏng — trả vector rỗng sẽ ghi vào kho những hàng vô nghĩa mà
            // không ai phát hiện ra cho tới lúc tìm kiếm trả về rác.
            None => Err(pai_rag::RagError::Unavailable(
                "chưa cấu hình nhà cung cấp nào cho mô hình nhúng".into(),
            )),
        }
    }

    async fn health(&self) -> bool {
        match self.current() {
            Some(inner) => inner.health().await,
            None => false,
        }
    }
}
