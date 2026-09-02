//! Read before edit.
//!
//! The rule: `edit` and `write` on an **existing** file only run if that file has been
//! `read` this session. It blocks exactly one failure: the model guessing the contents of a
//! file it never opened, then overwriting with what it imagined.
//!
//! Where the rule lives matters as much as what the rule says. It is **a middleware on the
//! pipeline**, not a field in a tool schema. So: the `edit` tool does not know this rule
//! exists, turning it off is removing a plugin rather than editing a tool, and any
//! file-writing tool written later is covered automatically with its author having to
//! remember nothing.

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
                // A broken path is the roots layer's business, not this rule's. Delegate
                // so it reports the real reason rather than "not read yet".
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
