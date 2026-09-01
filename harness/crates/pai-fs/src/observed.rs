//! Đọc trước khi sửa.
//!
//! Luật: `edit` và `write` lên một tệp **đã tồn tại** chỉ chạy nếu tệp đó đã được `read`
//! trong phiên này. Nó chặn đúng một kiểu hỏng: mô hình đoán nội dung một tệp nó chưa mở,
//! rồi ghi đè bằng thứ nó tưởng tượng ra.
//!
//! Chỗ đặt luật quan trọng ngang nội dung luật. Nó là **một middleware trên đường ống**,
//! không phải một trường trong schema tool. Vì thế: tool `edit` không biết luật này tồn
//! tại, tắt luật đi là gỡ một plugin chứ không phải sửa một tool, và một tool ghi tệp
//! viết sau này tự động chịu luật mà tác giả của nó không phải nhớ gì cả.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Middleware, Next};
use pai_tools::{PreDecision, PreExecute, PreRequest};

/// Những tệp đã được đọc trong phiên này.
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

/// Tool nào phải tuân luật, và đọc đường dẫn ở tham số nào.
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
                // Đường dẫn hỏng là việc của tầng gốc, không phải của luật này. Uỷ quyền
                // để nó báo đúng lý do thật thay vì báo "chưa đọc".
                return next.run(req).await;
            };
            // Tệp chưa tồn tại thì không có nội dung nào để đoán sai.
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
