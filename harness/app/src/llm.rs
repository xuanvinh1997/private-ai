//! One pointer to the active provider, shared by everything that talks to the model. Without it a switch
//! only half applies: sub-agents, `Rebuild` and model administration each captured an adapter at startup and
//! kept calling the old server silently. The fix is to hand out no copies -- everyone holds this `ActiveLlm`.

use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use futures::stream::BoxStream;
use pai_llm::{Capabilities, ChatRequest, LlmAdapter, LlmError, ModelAdmin, StreamChunk};

pub struct ActiveLlm {
    // Two layers of `Arc` are required: `arc-swap` needs `Arc<T>` with `T: Sized`, which `dyn LlmAdapter` is not.
    inner: ArcSwap<Arc<dyn LlmAdapter>>,
}

impl ActiveLlm {
    pub fn new(initial: Arc<dyn LlmAdapter>) -> ActiveLlm {
        ActiveLlm {
            inner: ArcSwap::from_pointee(initial),
        }
    }

    pub fn set(&self, next: Arc<dyn LlmAdapter>) {
        tracing::info!(provider = next.id(), "switched the active provider");
        self.inner.store(Arc::new(next));
    }

    pub fn current(&self) -> Arc<dyn LlmAdapter> {
        Arc::clone(&self.inner.load())
    }
}

#[async_trait]
impl LlmAdapter for ActiveLlm {
    /// A constant, not the underlying provider's id: `id` returns a `&str` borrowed from `self`, and the active
    /// adapter sits behind an `ArcSwap`. The real id is logged in [`ActiveLlm::set`].
    fn id(&self) -> &str {
        "đang-hoạt-động"
    }

    /// Bridge through a channel rather than returning the inner adapter's stream: `stream` returns a
    /// `BoxStream<'_>` borrowed from `&self` while the active adapter is an owned value pulled from an
    /// `ArcSwap`, and the self-referential alternative needs `unsafe`. One hand-off per chunk is cheap.
    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        // Pin the adapter before opening the stream: a turn must run entirely against one server.
        let adapter = self.current();
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let mut inner = adapter.stream(req);
            while let Some(chunk) = futures::StreamExt::next(&mut inner).await {
                if tx.send(chunk).await.is_err() {
                    // The receiver is gone; dropping `inner` here is the cancellation.
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
