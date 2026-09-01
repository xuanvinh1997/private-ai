//! Đăng ký là hiệu ứng gỡ lại được.
//!
//! Cordis cần `ctx.effect()` vì JavaScript không có destructor. Rust có, nên mặc định ở
//! đây là RAII: mọi hàm đăng ký trả về một guard, và thả guard là gỡ đăng ký.
//!
//! Vẫn cần một scope tường minh cho hai việc `Drop` không làm được:
//!
//! 1. **Dọn bất đồng bộ.** Đóng một client MCP, flush WAL, `await` một `JoinHandle`.
//! 2. **Thứ tự.** Cordis dọn theo LIFO. Rust thả field theo thứ tự khai báo và phần tử
//!    `Vec` từ đầu về cuối — ngược lại. Đây là loại sai khác không báo lỗi, chỉ chạy sai.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::BoxFuture;
use parking_lot::Mutex;

enum Disposer {
    Sync(Box<dyn FnOnce() + Send>),
    Async(Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>),
}

/// Sở hữu mọi đăng ký của một plugin — tương đương một fiber của Cordis.
pub struct EffectScope {
    label: &'static str,
    disposers: Mutex<Vec<(&'static str, Disposer)>>,
    children: Mutex<Vec<Arc<EffectScope>>>,
    disposed: AtomicBool,
    cancel: Mutex<Option<tokio_util::sync::CancellationToken>>,
}

impl EffectScope {
    pub fn new(label: &'static str) -> Arc<EffectScope> {
        Arc::new(EffectScope {
            label,
            disposers: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            disposed: AtomicBool::new(false),
            cancel: Mutex::new(None),
        })
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Một scope con. Con được dọn trước cha.
    pub fn child(self: &Arc<Self>, label: &'static str) -> Arc<EffectScope> {
        let child = EffectScope::new(label);
        self.children.lock().push(child.clone());
        child
    }

    pub fn defer(&self, label: &'static str, f: impl FnOnce() + Send + 'static) {
        self.disposers
            .lock()
            .push((label, Disposer::Sync(Box::new(f))));
    }

    pub fn defer_async<F, Fut>(&self, label: &'static str, f: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.disposers
            .lock()
            .push((label, Disposer::Async(Box::new(move || Box::pin(f())))));
    }

    /// Token huỷ sống đúng bằng scope này.
    ///
    /// Dựng lười và nhớ lại, nên mọi việc chạy dưới cùng một scope chia chung một token —
    /// gỡ scope là mọi thứ nó sinh ra dừng lại, không cần ai đi thu từng cái một.
    pub fn cancel_token(&self) -> tokio_util::sync::CancellationToken {
        let mut slot = self.cancel.lock();
        slot.get_or_insert_with(|| {
            let token = tokio_util::sync::CancellationToken::new();
            let owned = token.clone();
            self.defer("cancel", move || owned.cancel());
            token
        })
        .clone()
    }

    /// Giao một guard cho scope: nó sống đúng bằng plugin đã tạo ra nó.
    pub fn keep<G: Send + 'static>(&self, guard: G) {
        self.defer("keep", move || drop(guard));
    }

    /// Dọn. Gọi nhiều lần không sao. Một disposer hỏng không chặn những cái còn lại —
    /// dọn dở dang tệ hơn dọn có một chỗ hỏng.
    pub async fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        let children: Vec<_> = { self.children.lock().drain(..).collect() };
        for child in children.into_iter().rev() {
            Box::pin(child.dispose()).await;
        }
        let taken = { std::mem::take(&mut *self.disposers.lock()) };
        for (label, disposer) in taken.into_iter().rev() {
            match disposer {
                Disposer::Sync(f) => {
                    if let Err(err) = catch_unwind(AssertUnwindSafe(f)) {
                        tracing::error!(label, "disposer hoảng loạn: {err:?}");
                    }
                }
                Disposer::Async(f) => f().await,
            }
        }
    }
}

impl Drop for EffectScope {
    fn drop(&mut self) {
        if !self.disposed.load(Ordering::SeqCst) && !self.disposers.lock().is_empty() {
            tracing::warn!(
                label = self.label,
                "EffectScope bị thả mà chưa dispose() — phần dọn bất đồng bộ đã bị bỏ qua"
            );
        }
    }
}

/// Guard chung cho một đăng ký. Thả nó là gỡ đăng ký.
///
/// `#[must_use]` biến lỗi kinh điển của Cordis — quên disposer — thành cảnh báo lúc
/// biên dịch. Muốn đăng ký sống bằng plugin thì giao cho scope bằng `Context::keep`.
#[must_use = "thả guard ngay lập tức sẽ gỡ đăng ký; hãy giữ nó hoặc gọi ctx.keep(guard)"]
pub struct Guard(Option<Box<dyn FnOnce() + Send>>);

impl Guard {
    pub fn new(undo: impl FnOnce() + Send + 'static) -> Guard {
        Guard(Some(Box::new(undo)))
    }

    /// Bỏ guard đi và giữ đăng ký vĩnh viễn. Dùng cho những thứ thật sự sống bằng
    /// tiến trình; ở mọi chỗ khác, `keep` mới là câu trả lời đúng.
    pub fn leak(mut self) {
        self.0 = None;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(undo) = self.0.take() {
            undo();
        }
    }
}
