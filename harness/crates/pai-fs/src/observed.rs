//! Read before edit: `edit` and `write` on an existing file require a `read` this session,
//! so the model cannot overwrite a file it only guessed at. The rule is pipeline middleware,
//! not tool code, so disabling it removes a plugin and later tools are covered for free.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Middleware, Next};
use pai_tools::{PreDecision, PreExecute, PreRequest};

/// The files that have been read this session.
#[derive(Default)]
pub struct ReadLedger {
    seen: parking_lot::RwLock<HashSet<PathBuf>>,
}

impl ReadLedger {
    pub fn note_read(&self, path: &Path) {
        self.seen.write().insert(path.to_path_buf());
    }

    pub fn has_read(&self, path: &Path) -> bool {
        self.seen.read().contains(path)
    }
}

/// Which tools the rule applies to.
const GATED: &[&str] = &["edit", "write"];

pub struct ReadBeforeEdit {
    ledger: Arc<ReadLedger>,
    roots: crate::path::FileRoots,
}

impl ReadBeforeEdit {
    pub fn new(ledger: Arc<ReadLedger>, roots: crate::path::FileRoots) -> ReadBeforeEdit {
        ReadBeforeEdit { ledger, roots }
    }
}

impl Middleware<PreExecute> for ReadBeforeEdit {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        async move {
            if !GATED.contains(&req.name.as_str()) {
                return next.run(req).await;
            }
            let Some(raw) = req.arguments.get("file_path").and_then(|v| v.as_str()) else {
                return next.run(req).await;
            };
            let Ok(resolved) = self.roots.resolve_write(Path::new(raw)) else {
                // A broken path is the roots layer's business; delegate so it reports the real reason.
                return next.run(req).await;
            };
            // A file that does not exist yet has no contents to guess wrongly.
            if !resolved.exists() || self.ledger.has_read(&resolved) {
                return next.run(req).await;
            }
            PreDecision::Deny(format!(
                "Hãy dùng `read` để đọc {} trước khi sửa nó. Ghi đè một tệp chưa mở là \
                 ghi đè bằng nội dung phỏng đoán.",
                resolved.display()
            ))
        }
        .boxed()
    }
}
