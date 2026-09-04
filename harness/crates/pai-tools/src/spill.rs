//! The spill store: long output is kept whole rather than truncated.
//! Anything past the threshold goes to the store and the model gets a locator, so the
//! threshold decides how much the model reads, never what continues to exist.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::name::ToolName;

/// A ticket for retrieving the full text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpillRef {
    pub id: String,
    pub tool: String,
    /// Full-text size in Unicode characters, the same unit as the threshold.
    pub chars: usize,
    pub lines: usize,
}

impl SpillRef {
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "tool": self.tool, "chars": self.chars, "lines": self.lines })
    }
}

/// Where output not sent to the model is kept.
pub trait SpillStore: Send + Sync + 'static {
    /// Store the full text and return a ticket.
    fn spill(&self, tool: &ToolName, full: &str) -> SpillRef;

    /// Fetch the full text by id, the only part the model holds, since the whole ticket lives in `meta`.
    fn read_id(&self, id: &str) -> Option<String>;

    /// A convenience for the host, which still holds the whole ticket.
    fn read(&self, handle: &SpillRef) -> Option<String> {
        self.read_id(&handle.id)
    }
}

/// An in-memory implementation living as long as the session; a host needing more plugs into the same seam.
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

    fn read_id(&self, id: &str) -> Option<String> {
        self.entries.get(id).map(|entry| entry.clone())
    }
}
