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
    /// Đã cắt bớt để vừa ngân sách. `SearchMeta` có trường này từ đầu; `ReadMeta` thì
    /// không, nên giao diện không phân biệt được "đọc hết tệp" với "đọc phần đầu và phần
    /// cuối" — và một tệp bị cắt mà không nói ra thì người đọc kết luận "hết rồi" ở đúng
    /// chỗ lõi ngừng đọc.
    #[serde(default)]
    pub truncated: bool,
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
    /// Vé lấy lại toàn văn khi output đã bị cắt cho vừa ngân sách token.
    ///
    /// Mô hình lấy lại bằng tool `spill_read`; giao diện dùng nó để vẽ một lối xem đầy đủ.
    /// Thiếu trường này thì serde vứt lặng lẽ khoá `spill` mà tool đã ghi, và cả hai bên
    /// đều mất đường tới bản đầy đủ — mô hình thì còn `spill_read`, người dùng thì không
    /// còn gì.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill: Option<SpillMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalMeta>,
}

/// Vé lấy lại toàn văn. Mirror của `pai_tools::SpillRef`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpillMeta {
    pub id: String,
    pub tool: String,
    /// Kích thước toàn văn, tính bằng ký tự Unicode.
    pub chars: u64,
    pub lines: u64,
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

/// Quyền tool mà người dùng cấp cho **một lượt**.
///
/// Đi kèm từng tin nhắn chứ không phải là một thiết lập lưu lại, và đó là một quyết định
/// chứ không phải một thiếu sót: hạ quyền cho đúng một câu hỏi rồi nâng lại là cách người
/// ta thật sự dùng bộ chọn này, còn một thiết lập dính là thứ người dùng quên mất mình đã
/// đặt — rồi hoặc ngạc nhiên vì trợ lý không làm được gì, hoặc tưởng mình đang được che
/// chắn trong khi không.
///
/// Thứ tự các biến thể là thứ tự **nới dần**, và phép ánh xạ sang hạn chế thật nằm ở
/// `crate::scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolScope {
    /// Chỉ tool tự khai `mutating: false`.
    Read,
    /// Thêm tool sửa tệp; không có tool chạy lệnh.
    Write,
    /// Toàn bộ, kể cả quyền thi hành lệnh trên máy này.
    Shell,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    /// Gọi được tool không. Mô hình không gọi được tool thì coding agent vô dụng, nên
    /// giao diện phải nói ra trước khi người dùng chọn nhầm.
    pub tools: bool,
    /// Trò chuyện được.
    pub chat: bool,
    /// Nhúng được.
    ///
    /// Hai cờ này không loại trừ nhau, và đó là lý do có hai cờ chứ không phải một enum:
    /// `embedding == true && chat == false` là thứ **chỉ** nhúng được, và chỉ nhóm đó mới
    /// bị giấu khỏi bộ chọn mô hình hội thoại. Lọc theo `chat == true` thì chặt hơn nhưng
    /// sai hướng: một máy chủ Ollama đời cũ không có trường `capabilities` sẽ để lõi phải
    /// đoán theo tên, và một lần đoán trượt khi ấy làm biến mất một mô hình dùng được.
    pub embedding: bool,
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
    pub kind: ProjectKind,
    /// URL đã clone về. `None` là thư mục vốn có sẵn trên máy.
    pub origin: Option<String>,
}

impl ProjectView {
    /// `current` là `None` khi **chưa mở dự án nào** — một trạng thái hợp lệ, không phải
    /// một chỗ chưa điền. Truyền chuỗi rỗng thay cho nó thì mọi dự án đều không phải dự án
    /// đang mở, điều đó đúng, nhưng nó đúng vì tình cờ chứ không vì ai viết ra ý ấy.
    pub fn new(project: pai_project::Project, current: Option<&str>) -> ProjectView {
        ProjectView {
            is_current: current.is_some_and(|id| id == project.id),
            kind: project.kind.into(),
            origin: project.origin,
            id: project.id,
            name: project.name,
            path: project.path,
            last_opened_at: project.last_opened_at,
        }
    }
}

/// Hai enum cùng chuỗi wire, ở hai tầng.
///
/// Kho không được phép biết về giao diện, và giao diện không được phép phụ thuộc vào hình
/// dạng của kho — nên loại dự án tồn tại hai lần, và chỗ này là cây cầu. Một `From` mười
/// dòng rẻ hơn hẳn việc `pai-project` phải kéo theo `serde(rename_all)` của tầng trình bày.
impl From<pai_project::ProjectKind> for ProjectKind {
    fn from(kind: pai_project::ProjectKind) -> ProjectKind {
        match kind {
            pai_project::ProjectKind::Code => ProjectKind::Code,
            pai_project::ProjectKind::Docs => ProjectKind::Docs,
        }
    }
}

impl From<ProjectKind> for pai_project::ProjectKind {
    fn from(kind: ProjectKind) -> pai_project::ProjectKind {
        match kind {
            ProjectKind::Code => pai_project::ProjectKind::Code,
            ProjectKind::Docs => pai_project::ProjectKind::Docs,
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// Dự án hai loại, thư viện tài liệu, provider, và MCP.
//
// Bốn nhóm dưới đây được thêm cùng một lượt vì chúng là **một** thay đổi về sản phẩm:
// một dự án giờ có thể là mã nguồn hoặc tài liệu, và cái loại đó quyết định tool nào
// được cắm, màn hình nào mở ra, và mô hình nào trả lời. Tách chúng ra thành bốn đợt
// nghĩa là có một khoảng thời gian giao diện biết về `kind` mà lõi thì chưa.
// ───────────────────────────────────────────────────────────────────────────────

/// Một dự án là mã nguồn, hay là một chồng tài liệu.
///
/// Đây không phải một nhãn để lọc danh sách: nó chọn **tầng plugin** nào được cắm. Dự án
/// mã nguồn có `fs`/`shell`/`index`/`lsp`/`terminal`; dự án tài liệu có `rag` và không có
/// gì chạy được lệnh. Một dự án tài liệu không cần `bash`, và cấp cho nó `bash` "cho
/// tiện" là mở một đường thi hành lệnh vào một thư mục toàn tệp người ngoài gửi tới.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Code,
    Docs,
}

/// Tiến trình `git clone`, phát trên một `Channel` trong lúc lệnh đang chạy.
///
/// Một bản clone vài trăm megabyte không có thời hạn hợp lý nào, nên giao diện phải thấy
/// nó đang nhúc nhích. `percent` vắng mặt ở những pha mà git không đếm được — và một
/// thanh tiến trình đứng im ở 0% thì tệ hơn hẳn một dòng chữ nói "đang phân giải delta".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneProgress {
    /// Pha do git tự đặt tên: "Đang đếm đối tượng", "Đang nhận", "Đang phân giải delta".
    pub phase: String,
    pub percent: Option<u8>,
    /// Dòng thô, giữ lại cho khung "chi tiết" khi có sự cố.
    pub line: Option<String>,
    pub finished: bool,
    /// Thư mục đã clone xong. Chỉ có ở sự kiện cuối, và chỉ khi thành công.
    pub path: Option<String>,
    pub error: Option<String>,
}

/// Một tài liệu trong thư viện của dự án tài liệu.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub id: String,
    /// Đường dẫn tới bản sao trong kho của dự án, không phải chỗ người dùng lấy nó.
    pub path: String,
    pub title: String,
    /// `pdf`, `docx`, `markdown`, `text`, `html`, `csv`, `code`.
    pub format: String,
    pub bytes: u64,
    pub chunks: u32,
    /// Đã có vector chưa. `false` mà không có `error` nghĩa là **đang xếp hàng**, không
    /// phải hỏng: tìm bằng từ khoá vẫn chạy trong lúc chờ.
    pub embedded: bool,
    pub added_at: i64,
    pub error: Option<String>,
}

/// Tiến trình nạp tài liệu vào thư viện.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestProgress {
    /// Tệp đang xử lý.
    pub path: String,
    pub stage: String,
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    pub error: Option<String>,
}

/// Sức khoẻ của thư viện tài liệu, đủ để giao diện nói **vì sao** câu trả lời kém.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
    /// Mô hình nhúng đang dùng. `None` = chưa cấu hình được, và khi đó tìm kiếm lùi về
    /// **chỉ từ khoá** thay vì trả về rỗng.
    pub embedder: Option<String>,
    pub semantic_ready: bool,
    /// Câu tiếng Việt giải thích khi `semantic_ready` là `false`, **hoặc** khi thư viện
    /// trống trong lúc thư mục thì không.
    pub reason: Option<String>,
    /// Thư mục tài liệu của người dùng.
    ///
    /// Giao diện phải chỉ ra được nó. Câu hỏi "vì sao không thấy tệp nào" bắt đầu bằng
    /// việc người dùng kiểm lại họ đã chỉ vào đâu, và một màn hình không nói ra thư mục
    /// nào đang được quét thì không trả lời được câu ấy.
    pub root: String,
    pub files_seen: u32,
    /// Bỏ qua vì chạm trần — kích thước tệp hoặc trần số tệp.
    pub files_skipped: u32,
    pub unreadable: u32,
    /// Còn trong thư mục nhưng người dùng đã bỏ khỏi thư viện.
    pub excluded: u32,
    /// Lần quét gần nhất, epoch mili-giây. `None` là **chưa quét lần nào** — khác hẳn
    /// "quét rồi và không có gì", và giao diện phải phân biệt hai câu đó.
    pub scanned_at: Option<i64>,
    /// Đang quét: `(xong, tổng)`. `None` là không có lượt quét nào đang chạy.
    pub scanning: Option<ScanProgress>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub done: u32,
    pub total: u32,
}

/// Một đoạn khớp, đủ để dựng thẻ trích dẫn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentHit {
    pub document_id: String,
    pub title: String,
    pub path: String,
    pub ordinal: u32,
    pub text: String,
    pub score: f32,
    /// `keyword`, `semantic`, hoặc `both` — người đọc cần biết vì sao đoạn này được chọn.
    pub matched_by: String,
}

/// Một provider đã cấu hình, như giao diện thấy nó. **Không mang khoá API.**
///
/// `has_key` thay cho chính cái khoá: giao diện chỉ cần biết ô nhập nên hiện "đã đặt" hay
/// hiện trống, và một khoá đi qua IPC là một khoá nằm trong log của mọi công cụ gỡ lỗi
/// đang mở.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    /// `ollama` hoặc `openai`.
    pub kind: String,
    pub base_url: String,
    pub has_key: bool,
    pub enabled: bool,
    /// Endpoint không rời loopback: dữ liệu không đi đâu cả, và giao diện nói ra điều đó.
    pub on_device: bool,
    /// Đang dùng để **trò chuyện**.
    pub active_chat: bool,
    /// Đang dùng để **nhúng tài liệu**.
    ///
    /// Hai vai tách hẳn nhau, và đó không phải một tuỳ chọn cho người thích nghịch: mô
    /// hình nhúng và mô hình hội thoại là hai loại mô hình khác nhau, chạy trên hai
    /// endpoint khác nhau, và cách ghép hợp lý nhất trong thực tế lại chính là ghép chéo —
    /// nhúng bằng một mô hình nhỏ chạy tại chỗ (miễn phí, không gửi tài liệu đi đâu) trong
    /// khi trò chuyện bằng một mô hình lớn từ xa. Buộc chúng dùng chung một provider là
    /// loại bỏ đúng cấu hình mà phần lớn người dùng muốn.
    pub active_embedding: bool,
    /// Mô hình hội thoại đang chọn cho provider này.
    pub model: Option<String>,
    /// Mô hình nhúng đang chọn cho provider này.
    pub embedding_model: Option<String>,
}

/// Cấu hình nhúng đang có hiệu lực, gộp từ provider và mô hình.
///
/// Một kiểu riêng thay vì bắt giao diện tự ghép từ danh sách provider: câu hỏi "tài liệu
/// của tôi đang được nhúng bằng cái gì, và nó có chạy không" là một câu hỏi, và trả lời nó
/// bằng cách bắt người đọc tự lọc một danh sách là bắt họ làm việc của máy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSetting {
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    /// Tài liệu không rời khỏi máy này khi nhúng.
    pub on_device: bool,
    /// Câu tiếng Việt nói vì sao chưa dùng được, khi chưa dùng được.
    pub reason: Option<String>,
}

/// Kết quả thử **nhúng thật một câu**, không phải liệt kê mô hình.
///
/// Liệt kê mô hình không trả lời được câu hỏi thật: `/api/tags` của Ollama trả về mọi mô
/// hình, và không có gì trong đó nói mô hình nào nhúng được. Cách duy nhất biết chắc là
/// gửi một câu đi và xem có vector trả về không — nên phép thử này làm đúng thế, và báo
/// lại số chiều.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingProbe {
    pub ok: bool,
    pub message: String,
    /// Số chiều đo được từ vector thật trả về.
    pub dimensions: Option<usize>,
}

/// Một mục dựng sẵn trong danh mục provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub needs_key: bool,
    pub on_device: bool,
    pub default_model: Option<String>,
    /// Chỗ lấy khoá, hoặc chỗ tải máy chủ về.
    pub homepage: String,
    pub hint: String,
}

/// Kết quả thử một cấu hình provider **trước khi lưu nó**.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbe {
    pub ok: bool,
    pub message: String,
    pub models: Vec<ModelChoice>,
}

/// Một server MCP như giao diện thấy nó.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub name: String,
    /// `stdio` hoặc `http`.
    pub transport: String,
    /// Dòng lệnh hoặc URL, rút gọn để hiện trên một dòng.
    pub target: String,
    pub enabled: bool,
    /// `connected`, `connecting`, `failed`, `disabled`.
    pub state: String,
    /// Tên tool đã cắm, đã mang tiền tố `ext.<name>.`.
    pub tools: Vec<String>,
    pub error: Option<String>,
}

/// Một biến môi trường mà một mục danh mục cần người dùng điền.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpEnvVar {
    pub key: String,
    pub label: String,
    pub required: bool,
    /// Che khi gõ, và không gửi ngược ra giao diện sau khi lưu.
    pub secret: bool,
}

/// Một server dựng sẵn mà người dùng cắm bằng một cú bấm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCatalogEntry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<McpEnvVar>,
    pub homepage: String,
    /// Cần gì có sẵn trên máy: `node`, `python`, `docker`. Giao diện cảnh báo trước khi
    /// người dùng bấm cắm rồi nhìn một server `failed` mà không hiểu vì sao.
    pub requires: Vec<String>,
}

/// Một đỉnh trong đồ thị mã nguồn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeView {
    pub id: String,
    pub name: String,
    /// `function`, `method`, `struct`, `class`, `trait`, `interface`, `enum`, `module`,
    /// `constant`, `type`.
    pub kind: String,
    pub path: String,
    pub line: u32,
}

/// Một cạnh. `kind`: `calls`, `imports`, `contains`, `implements`, `extends`, `references`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeView {
    pub src: String,
    pub dst: String,
    pub kind: String,
}

/// Một lát cắt của đồ thị, đủ nhỏ để vẽ.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView {
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    /// Đã cắt bớt để vẽ được. Một đỉnh có bốn trăm cạnh thì vẽ ra là một quả cầu đen.
    pub truncated: bool,
}

/// Tình trạng chỉ mục mã nguồn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub files: u32,
    pub symbols: u32,
    pub edges: u32,
    /// `(ngôn ngữ, số tệp)`, nhiều trước.
    pub languages: Vec<(String, u32)>,
    /// Lần quét gần nhất, epoch mili-giây.
    pub scanned_at: Option<i64>,
}

/// Cấu hình provider **gửi lên từ giao diện**.
///
/// `api_key` mang một ngữ nghĩa ba trạng thái mà kiểu `Option<String>` nói đúng nhưng
/// người đọc dễ lướt qua: `None` là **giữ nguyên khoá cũ**, `Some("")` là **xoá khoá**, và
/// `Some(k)` là đặt khoá mới. Giao diện không bao giờ nhận lại khoá, nên nó không thể gửi
/// lại khoá — và một `save` làm mất khoá mỗi lần người dùng sửa tên provider là lỗi chắc
/// chắn xảy ra nếu chỗ này gộp `None` với `Some("")`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInputWire {
    pub id: Option<String>,
    pub name: String,
    /// `ollama` hoặc `openai`.
    pub kind: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub model: Option<String>,
    pub embedding_model: Option<String>,
}

/// Một server MCP gửi lên từ giao diện.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpServerInputWire {
    pub name: String,
    /// `stdio` hoặc `http`.
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub enabled: bool,
}

/// Vòng giam tiến trình, như giao diện thấy nó.
///
/// Cùng tinh thần với [`pai_sandbox::Enforcement`]: đây là **báo cáo sự thật, không phải
/// lời hứa**. `mode` nói kernel đang thi hành tới đâu, `reason` nói nó thủng ở chỗ nào khi
/// nó thủng. Một màn hình quyền hạn im lặng về chuyện này dạy người dùng tin vào một
/// ranh giới có thể không tồn tại trên máy của họ.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// `full`, `partial`, hoặc `none`.
    pub mode: String,
    /// Vì sao chỉ thủng hoặc vì sao không có gì. `None` khi `mode` là `full`.
    pub reason: Option<String>,
    /// Thư mục lệnh được phép ghi vào.
    pub writable_roots: Vec<String>,
    /// `macos`, `linux`, `windows` — mức giam khác nhau theo nền tảng, và người đọc cần
    /// biết họ đang đứng trên nền nào để hiểu con số kia.
    pub platform: String,
}

/// Một hook đang cài, chỉ đọc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRow {
    pub command: String,
    /// Tool mà hook này áp vào. Rỗng = mọi tool.
    pub tools: Vec<String>,
    /// Hạn giờ riêng, giây. `None` = dùng mặc định của lõi.
    pub timeout_secs: Option<u64>,
    /// Lớp cấu hình đã khai nó — bản dựng sẵn hay tệp vá của người dùng.
    pub origin: String,
}
