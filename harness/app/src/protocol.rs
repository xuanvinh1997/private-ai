//! Hợp đồng giữa lõi và giao diện.
//!
//! Bản sao TypeScript nằm ở `ui/src/lib/protocol.ts`. Hai đầu khớp bằng tay cho tới khi
//! có bước sinh mã, nên **mọi thay đổi ở đây phải kèm thay đổi bên kia trong cùng một
//! commit** — lệch nhau thì hỏng lúc chạy chứ không lúc biên dịch.
//!
//! `rename_all = "snake_case"` chỉ đổi tên *biến thể*; tên trường vốn đã snake_case nên
//! đi thẳng qua wire. Riêng những kiểu giao diện đọc bằng camelCase thì khai báo rõ.

// Đây là hợp đồng nối dây, không phải mã ứng dụng: một biến thể chưa có nơi dựng nghĩa
// là phần lõi tương ứng chưa được nối vào, chứ không phải nó thừa. Giao diện đã đọc đủ
// cả hình dạng này rồi, nên cắt bớt ở đây chỉ làm hai đầu lệch nhau.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Một hunk diff.
///
/// `old_text: None` nghĩa là **tệp mới**, không phải "không có gì đổi".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
    /// Số dòng đầu trong tệp thật. Vắng thì giao diện đánh số từ 1, và đó là số *trong
    /// hunk* chứ không phải trong tệp — một sai lệch âm thầm. Tính được thì gửi kèm.
    pub old_start: Option<u32>,
    pub new_start: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadLine {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMeta {
    pub path: String,
    pub offset: u32,
    pub lines: Vec<ReadLine>,
    pub total_lines: u32,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchGroup {
    pub path: String,
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchShape {
    Matches,
    Paths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMeta {
    pub shape: SearchShape,
    /// Kết quả bị cắt bớt để hiển thị. Bản đầy đủ nằm trong spill store, không mất.
    pub truncated: bool,
    pub total: u32,
    pub groups: Option<Vec<SearchGroup>>,
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalMeta {
    pub command: String,
    pub cwd: Option<String>,
    pub output: String,
    /// Chạy nền thì chưa có mã thoát — điều đó không có nghĩa là treo.
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub background: bool,
    pub job_id: Option<String>,
}

/// Phần đi kèm kết quả tool để giao diện vẽ thẻ giàu.
///
/// dsh khai báo `presentCall`/`presentResult` ở phía host nhưng bản web **không dùng**:
/// nó đọc thẳng `meta`. Chép đúng chỗ đó — giao diện tự render từ sự kiện thô, không có
/// API trình bày nào ở giữa để lệch pha.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<DiffHunk>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalMeta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub status: TodoStatus,
}

/// Quyết định duyệt. Đúng hai giá trị: **không có "nhớ lựa chọn"** trong từ vựng này.
/// Một lần đồng ý là một lần, không phải một chính sách.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowedOnce,
    Rejected,
}

/// Một sự kiện trong đời một lượt.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Một mẩu văn bản của trợ lý.
    ///
    /// Đây là sự kiện dày nhất, và mỗi lần vượt biên IPC của Tauri đắt hơn hẳn một
    /// signal của Qt — nên token được **gộp ở phía Rust** trước khi gửi, không phát
    /// từng cái một.
    Token {
        text: String,
    },
    Progress {
        label: String,
        detail: Option<String>,
    },
    Notice {
        message: String,
    },
    ToolStart {
        call_id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolEnd {
        call_id: String,
        name: String,
        /// Lỗi cấp tool — mô hình đọc được. Không phải panic.
        is_error: bool,
        preview: String,
        // Đóng hộp vì nó lớn hơn mọi biến thể khác cộng lại, mà `Token` mới là cái
        // được dựng hàng nghìn lần một lượt. Trên dây không có gì đổi.
        meta: Option<Box<ToolMeta>>,
    },
    /// Diff *dự kiến*, phát ngay khi tool bắt đầu: người dùng thấy thay đổi trước khi
    /// nó xảy ra. Giao diện cũng tự suy được từ `args`, nên đây chỉ là đường tắt cho
    /// tool mà args không đủ để dựng diff.
    Diff {
        call_id: String,
        diffs: Vec<DiffHunk>,
    },
    /// Toàn bộ danh sách việc, mỗi lần một bản đầy đủ — giao diện không phải gấp trạng
    /// thái, và không có cách nào để hai bên lệch nhau.
    Todo {
        items: Vec<TodoItem>,
    },
    /// Lõi hỏi ngược giao diện. Không trả lời được là từ chối.
    ApprovalRequest {
        request_id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
        reason: Option<String>,
        timeout_ms: Option<u64>,
    },
    /// Rút lại câu hỏi vì lượt đã bị huỷ. Giao diện đóng hộp thoại.
    ApprovalCancel {
        request_id: String,
    },
    Final {
        message_id: String,
    },
    Error {
        message: String,
    },
}

/// Một phiên trong thanh bên.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// Epoch mili-giây.
    pub updated_at: i64,
    /// Câu cuối cùng đã nói trong phiên. `None` khi phiên chưa nói gì — và `None` phải
    /// làm hàng đó **một dòng**, không phải hai dòng với dòng dưới trống.
    pub preview: Option<String>,
}

impl SessionSummary {
    pub fn from_header(header: pai_session::SessionHeader) -> SessionSummary {
        SessionSummary::with_preview(header, None)
    }

    pub fn with_preview(
        header: pai_session::SessionHeader,
        preview: Option<String>,
    ) -> SessionSummary {
        SessionSummary {
            preview,
            // Phiên chưa có tiêu đề vẫn phải hiện được trong danh sách; một dòng trống là
            // một dòng không bấm được.
            title: header.title.unwrap_or_else(|| "Phiên mới".to_string()),
            id: header.id,
            updated_at: header.updated_at,
        }
    }
}

/// Một node trong bản ghi đã lưu, dựng lại từ sổ tay phiên.
///
/// Cùng từ vựng `kind` với `ConversationNode` bên giao diện, nên sổ đăng ký renderer
/// dùng lại nguyên vẹn — bản ghi nạp lại và lượt đang chạy vẽ bằng cùng một mã.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryNode {
    User {
        id: String,
        text: String,
        created_at: i64,
    },
    Assistant {
        id: String,
        text: String,
        created_at: i64,
    },
    Tool {
        id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
        is_error: bool,
        preview: String,
        // Đóng hộp vì cùng lý do như `AgentEvent::ToolEnd`: nó lớn hơn hai biến thể kia
        // cộng lại, mà một bản ghi dài thì phần lớn là message chứ không phải thẻ tool.
        meta: Option<Box<ToolMeta>>,
        created_at: i64,
    },
}

/// Một mô hình mà máy chủ đang có.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    /// Gọi được tool không. Mô hình không gọi được tool thì coding agent vô dụng, nên
    /// giao diện phải nói ra trước khi người dùng chọn nhầm.
    pub tools: bool,
    pub context_window: Option<u64>,
}

/// Một dự án trong thanh bên.
///
/// `is_current` là thứ giao diện cần mà [`pai_project::Project`] không có: kho không biết
/// dự án nào đang mở, và nhét trạng thái lúc chạy vào một hàng đã lưu là cách nó bị ghi
/// xuống đĩa rồi sai ở lần khởi động sau.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_opened_at: i64,
    pub is_current: bool,
}

impl ProjectView {
    pub fn new(project: pai_project::Project, current: &str) -> ProjectView {
        ProjectView {
            is_current: project.id == current,
            id: project.id,
            name: project.name,
            path: project.path,
            last_opened_at: project.last_opened_at,
        }
    }
}
