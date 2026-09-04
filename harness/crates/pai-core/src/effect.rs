//! Registration is an undoable effect: registering returns a guard, dropping it unregisters.
//! An explicit scope covers the two things `Drop` cannot do: async cleanup, and LIFO order
//! (Rust drops fields and `Vec` elements front to back, the opposite of what we need).

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

/// Owns every registration made by one plugin.
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

    /// A child scope. Children are disposed before their parent.
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

    /// Cancellation token living exactly as long as this scope; memoised so the scope shares one.
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

    /// Hand a guard to the scope, so it lives exactly as long as the plugin that made it.
    pub fn keep<G: Send + 'static>(&self, guard: G) {
        self.defer("keep", move || drop(guard));
    }

    /// Dispose; idempotent, and one failing disposer must not abort the remaining cleanup.
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
                        tracing::error!(label, "disposer panicked: {err:?}");
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
                "EffectScope dropped without dispose(); async cleanup was skipped"
            );
        }
    }
}

/// Guard for one registration: dropping it unregisters, and `#[must_use]` catches a forgotten one.
#[must_use = "dropping the guard immediately unregisters; hold it or call ctx.keep(guard)"]
pub struct Guard(Option<Box<dyn FnOnce() + Send>>);

impl Guard {
    pub fn new(undo: impl FnOnce() + Send + 'static) -> Guard {
        Guard(Some(Box::new(undo)))
    }

    /// Keep the registration forever; only for things that outlive every scope, else use `keep`.
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
