//! Danh mục mô hình: cái mà nửa quản trị của một provider nói về kho của nó.
//!
//! Port `core/schemas.py::ModelInfo` + `ModelState`. Không có gì ở đây đi vào một request
//! suy luận — nó phục vụ màn hình quản lý mô hình và bảng cho thuê VRAM.

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;

/// Mô hình đang ở đâu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// Máy chủ từ xa có nó, nhưng không có khái niệm nạp/nhả. Provider OpenAI-compatible
    /// luôn ở trạng thái này.
    Installed,
    /// Đã tải về đĩa, chưa nằm trong VRAM.
    Unloaded,
    /// Đang thường trú trong VRAM.
    Loaded,
}

/// Một mô hình trong kho.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub state: ModelState,
    /// Kích thước trên đĩa.
    pub size_bytes: u64,
    /// VRAM đo được khi đang thường trú; 0 khi không.
    pub vram_bytes: u64,
    pub quantization: Option<String>,
    /// Dấu thời gian ISO-8601 nguyên văn từ máy chủ. Giữ chuỗi chứ không parse: chỗ này
    /// chỉ hiện ra màn hình, và một dấu thời gian lệch chuẩn không đáng để làm hỏng cả
    /// danh sách mô hình.
    pub modified_at: Option<String>,
    pub capabilities: Capabilities,
}

impl ModelInfo {
    /// VRAM mà một lượt cho thuê nên giữ chỗ: số đo được, nếu không thì kích thước tệp
    /// cộng biên. Port `ModelAdmin.required_bytes` (`admin.py:172-174`).
    pub fn required_bytes(&self, overhead_ratio: f64) -> u64 {
        if self.vram_bytes > 0 {
            return self.vram_bytes;
        }
        let ratio = overhead_ratio.max(1.0);
        (self.size_bytes as f64 * ratio).ceil() as u64
    }
}

/// Một mô hình đang thường trú, theo `/api/ps`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunningModel {
    pub name: String,
    pub size_bytes: u64,
    pub vram_bytes: u64,
    /// Khi nào Ollama sẽ nhả nó, ISO-8601 nguyên văn.
    pub expires_at: Option<String>,
}

/// Chi tiết từ `/api/show`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelDetails {
    pub capabilities: Capabilities,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
}

/// Một dòng tiến trình của `/api/pull`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PullProgress {
    /// Câu trạng thái của Ollama, ví dụ `"pulling 8934d96d3f08"`.
    pub status: String,
    pub digest: Option<String>,
    pub total: Option<u64>,
    pub completed: Option<u64>,
}

impl PullProgress {
    /// 0.0–1.0 cho một dòng. Port `pull_fraction` (`admin.py:31-40`): thiếu số thì trả 0,
    /// **không** trả `None`, vì thanh tiến trình cần một con số ở mọi dòng.
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
