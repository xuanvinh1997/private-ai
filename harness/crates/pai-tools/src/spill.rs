//! Kho tràn: output dài được **giữ nguyên vẹn**, không bị cắt cụt.
//!
//! Bản Python cắt cứng ở 6000 ký tự (`adapter.py:22,64`) và phần dư biến mất — không ai
//! đọc lại được, kể cả người dùng đang ngồi trước màn hình. Chuyện đó hỏng theo cách tệ
//! nhất: tool đã làm xong việc, dữ liệu đã có, rồi bị vứt trên đường về.
//!
//! Ở đây, phần vượt ngưỡng được cất vào kho và mô hình nhận một locator. Ngưỡng chỉ quyết
//! định **mô hình đọc bao nhiêu**, không quyết định **cái gì còn tồn tại**.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::name::ToolName;

/// Vé lấy lại toàn văn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpillRef {
    pub id: String,
    pub tool: String,
    /// Kích thước toàn văn, tính bằng ký tự Unicode — cùng đơn vị với ngưỡng.
    pub chars: usize,
    pub lines: usize,
}

impl SpillRef {
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "tool": self.tool, "chars": self.chars, "lines": self.lines })
    }
}

/// Nơi cất phần output không gửi cho mô hình.
pub trait SpillStore: Send + Sync + 'static {
    /// Cất toàn văn, trả về vé.
    fn spill(&self, tool: &ToolName, full: &str) -> SpillRef;
    /// Lấy lại toàn văn. `None` nếu vé không còn giá trị.
    fn read(&self, handle: &SpillRef) -> Option<String>;
}

/// Bản cài đặt trong bộ nhớ, sống bằng phiên.
///
/// Đủ cho một phiên desktop: cái tràn ra là kết quả tool của chính lượt đang chạy, và nó
/// hết ý nghĩa khi phiên đóng. Một host cần lâu hơn thì cắm bản của mình vào cùng seam.
#[derive(Default)]
pub struct MemorySpillStore {
    entries: DashMap<String, String>,
    next: AtomicU64,
}

impl MemorySpillStore {
    pub fn new() -> Arc<MemorySpillStore> {
        Arc::new(MemorySpillStore::default())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl SpillStore for MemorySpillStore {
    fn spill(&self, tool: &ToolName, full: &str) -> SpillRef {
        let id = format!("spill-{}", self.next.fetch_add(1, Ordering::Relaxed));
        self.entries.insert(id.clone(), full.to_string());
        SpillRef {
            id,
            tool: tool.as_str().to_string(),
            chars: full.chars().count(),
            lines: full.lines().count(),
        }
    }

    fn read(&self, handle: &SpillRef) -> Option<String> {
        self.entries.get(&handle.id).map(|entry| entry.clone())
    }
}
