//! Model catalogue: what a provider's admin half says about its store.
//! Nothing here reaches an inference request; it feeds the model management screen
//! and the VRAM lease table.

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;

/// Where a model currently is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// A remote server has it but has no load/unload notion; OpenAI-compatible providers stay here.
    Installed,
    /// Downloaded to disk, not in VRAM.
    Unloaded,
    /// Resident in VRAM.
    Loaded,
}

/// One model in the store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub state: ModelState,
    /// Size on disk.
    pub size_bytes: u64,
    /// Measured VRAM while resident; 0 otherwise.
    pub vram_bytes: u64,
    pub quantization: Option<String>,
    /// ISO-8601 timestamp verbatim from the server; kept as a string so an off-spec value cannot break the whole list.
    pub modified_at: Option<String>,
    pub capabilities: Capabilities,
}

impl ModelInfo {
    /// VRAM a lease should reserve: the measured figure, else file size plus margin. Ports `ModelAdmin.required_bytes`.
    pub fn required_bytes(&self, overhead_ratio: f64) -> u64 {
        if self.vram_bytes > 0 {
            return self.vram_bytes;
        }
        let ratio = overhead_ratio.max(1.0);
        (self.size_bytes as f64 * ratio).ceil() as u64
    }
}

/// A resident model, per `/api/ps`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunningModel {
    pub name: String,
    pub size_bytes: u64,
    pub vram_bytes: u64,
    /// When Ollama will release it, ISO-8601 verbatim.
    pub expires_at: Option<String>,
}

/// Details from `/api/show`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelDetails {
    pub capabilities: Capabilities,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
}

/// One progress line from `/api/pull`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PullProgress {
    /// Ollama's status sentence, e.g. `"pulling 8934d96d3f08"`.
    pub status: String,
    pub digest: Option<String>,
    pub total: Option<u64>,
    pub completed: Option<u64>,
}

impl PullProgress {
    /// 0.0-1.0 for one line; ports `pull_fraction` - a missing number gives 0, not `None`, because the progress bar needs a number on every line.
    pub fn fraction(&self) -> f32 {
        let (Some(total), Some(completed)) = (self.total, self.completed) else {
            return 0.0;
        };
        if total == 0 {
            return 0.0;
        }
        (completed as f32 / total as f32).clamp(0.0, 1.0)
    }
}
