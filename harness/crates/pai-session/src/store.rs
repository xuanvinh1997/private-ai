//! Seam lưu bền, và seam đặt tiêu đề.
//!
//! Theo đúng khuôn của `pai-core`: khoá là một marker type không khởi tạo được, giá trị
//! là một trait object. Consumer viết `ctx.require::<Sessions>()` và không bao giờ nhắc
//! tên bản cài đặt — đổi SQLite sang JSONL hay sang một kho từ xa là đổi một dòng cắm
//! provider, không phải sửa call site.

use async_trait::async_trait;
use pai_core::ServiceKey;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::event::{SESSION_FORMAT_VERSION, Seq, SessionEventEnvelope};
use crate::log::SessionLog;

/// Danh tính công khai của một phiên. UUID v7 để sắp theo thời gian tạo.
pub type SessionId = String;

pub fn new_session_id() -> SessionId {
    uuid::Uuid::now_v7().to_string()
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Subagent,
}

impl Origin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Origin::Subagent => "subagent",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Origin> {
        match raw {
            "subagent" => Some(Origin::Subagent),
            _ => None,
        }
    }
}

/// Metadata của phiên. Tách khỏi sổ có chủ ý: sổ chỉ-ghi-thêm không có chỗ cho những
/// trường đổi được như tiêu đề.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionHeader {
    pub id: SessionId,
    pub format_version: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub title: Option<String>,
    /// Tuyệt đối, đã canonicalize. `None` = phiên không gắn thư mục nào.
    pub cwd: Option<String>,
    pub parent_session: Option<SessionId>,
    /// Bao nhiêu sự kiện đầu là kế thừa từ phiên cha. Ranh giới fork gốc, bền vững —
    /// khác với "đã phát lại bao nhiêu trong vòng đời này", vốn là chuyện lúc chạy.
    pub seed_length: Option<u64>,
    pub origin: Option<Origin>,
    pub delegation_depth: Option<u32>,
    pub agent_preset: Option<String>,
}

/// Yêu cầu tạo phiên. `id` để trống thì kho tự sinh.
#[derive(Clone, Debug, Default)]
pub struct NewSession {
    pub id: Option<SessionId>,
    pub cwd: Option<String>,
    pub parent_session: Option<SessionId>,
    pub seed_length: Option<u64>,
    pub origin: Option<Origin>,
    pub delegation_depth: Option<u32>,
    pub agent_preset: Option<String>,
}

impl NewSession {
    pub fn in_dir(cwd: impl Into<String>) -> NewSession {
        NewSession {
            cwd: Some(cwd.into()),
            ..NewSession::default()
        }
    }

    pub(crate) fn format_version(&self) -> i64 {
        SESSION_FORMAT_VERSION
    }
}

/// Kho phiên.
///
/// `append` nhận **một lô**, không phải một sự kiện. Mảnh stream đến rất dày, và một
/// transaction cho mỗi mảnh là cách chắc chắn nhất để biến ổ đĩa thành nút cổ chai.
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn create(&self, spec: NewSession) -> Result<SessionHeader>;

    /// Mới nhất trước.
    async fn list(&self, limit: Option<u32>) -> Result<Vec<SessionHeader>>;

    async fn header(&self, id: &str) -> Result<SessionHeader>;

    /// Lô phải bắt đầu đúng ở `last_seq + 1` của phiên. Kho từ chối nếu không —
    /// đây là chốt chặn cuối cho bất biến "seq liền mạch" khi có hai tiến trình cùng ghi.
    async fn append(&self, id: &str, events: Vec<SessionEventEnvelope>) -> Result<()>;

    /// Đọc lại từ đầu, theo thứ tự seq.
    async fn load(&self, id: &str) -> Result<Vec<SessionEventEnvelope>>;

    /// Số hàng thật sự nằm trong bảng — khác số sự kiện, vì nhiều mảnh stream chung một
    /// hàng. Dùng để đo, và để bài kiểm chứng gói mảnh có cái mà khẳng định.
    async fn row_count(&self, id: &str) -> Result<u64>;

    async fn set_title(&self, id: &str, title: &str) -> Result<()>;

    /// Câu cuối cùng đã nói trong mỗi phiên, để làm dòng phụ trong danh sách.
    ///
    /// Nhận **cả lô** chứ không từng phiên một: danh sách phiên hỏi cho tất cả cùng lúc,
    /// và một vòng lặp gọi hàm async cho từng phiên là một vòng lặp giành khoá kho đúng
    /// bằng số phiên. Phiên chưa nói gì thì vắng mặt trong kết quả — không có dòng phụ
    /// vẫn đọc được, dòng phụ bịa thì không.
    async fn previews(&self, ids: &[String]) -> Result<HashMap<String, String>>;

    /// Xoá hẳn một phiên và toàn bộ sự kiện của nó.
    ///
    /// Đây **không** phá bất biến chỉ-ghi-thêm: bất biến đó nói về việc sửa lịch sử bên
    /// trong một phiên, còn đây là người dùng vứt cả phiên đi. Hai chuyện khác nhau, và
    /// gộp chúng lại nghĩa là người dùng không bao giờ dọn được thứ họ không muốn giữ.
    async fn delete(&self, id: &str) -> Result<()>;
}

/// Seam kho phiên.
pub enum Sessions {}

impl ServiceKey for Sessions {
    type Api = dyn SessionStore;
    const NAME: &'static str = "sessions";
}

/// Seam đặt tiêu đề phiên.
///
/// Tách khỏi kho vì nó là một chính sách, không phải một cách lưu: một bản cài đặt sẽ gọi
/// mô hình, một bản khác cắt câu đầu, một bản khác nữa để người dùng tự gõ.
#[async_trait]
pub trait SessionTitler: Send + Sync + 'static {
    /// `None` = chưa đủ căn cứ để đặt tên. Đó là một câu trả lời hợp lệ, không phải lỗi.
    async fn title(&self, log: &SessionLog) -> Result<Option<String>>;
}

pub enum SessionTitle {}

impl ServiceKey for SessionTitle {
    type Api = dyn SessionTitler;
    const NAME: &'static str = "session.title";
}

/// Provider duy nhất của v0.1: chưa đặt tên gì cả.
///
/// Có mặt để seam tồn tại từ đầu. Consumer viết đúng một lần với `Option<String>`, và bản
/// gọi mô hình sau này cắm vào mà không ai phải sửa gì.
pub struct NoTitle;

#[async_trait]
impl SessionTitler for NoTitle {
    async fn title(&self, _log: &SessionLog) -> Result<Option<String>> {
        Ok(None)
    }
}

/// Ranh giới fork: `seq` **bao gồm cả nó**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boundary(pub Seq);
