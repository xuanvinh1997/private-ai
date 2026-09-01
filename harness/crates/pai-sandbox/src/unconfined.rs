//! Provider cho những máy chưa giam được — hôm nay là Windows.
//!
//! Đây **không phải** một bản giả. Nó không bọc argv, không sinh hồ sơ, và không bao giờ
//! trả về `Enforcement::Full`. Việc duy nhất nó làm là trả lời câu hỏi "có giam được
//! không" bằng "không, vì lý do này" — và câu trả lời đó có giá trị, vì im lặng thì hộp
//! thoại duyệt không phân biệt được "chưa ai cắm sandbox" với "đã cắm và nó nói không".
//!
//! Vì sao Windows chưa có: đã khảo sát bốn primitive và chỉ **một** cái khả thi.
//!
//! - **Restricted token** (`CreateRestrictedToken` với `WRITE_RESTRICTED` + SID tổng hợp
//!   cho workspace + Job Object) — khả thi, không cần quyền quản trị, và là thứ cả dsh
//!   lẫn Codex CLI chọn. Nhưng nó chỉ giao với quyền **ghi**: đọc, mạng và khả năng nhìn
//!   thấy tiến trình khác đều không bị hạn chế, `Everyone` bắt buộc phải nằm trong danh
//!   sách hạn chế (bỏ ra thì DLL init chết `0xC0000142`) nên mọi đối tượng NTFS cấp ghi
//!   cho `Everyone` vẫn ghi được, và hard link NTFS alias cùng một file object qua nhiều
//!   đường dẫn. Nghĩa là khi nó có mặt, nó phải báo cáo `Partial`, không bao giờ `Full`.
//! - **AppContainer** — mặc định từ chối *đọc*. Một coding agent phải đọc repo,
//!   toolchain, cấu hình git và cache phụ thuộc; đục đủ lỗ cho nó chạy thì ranh giới
//!   không còn nghĩa. Ngoài ra capability phải khai trước, mà agent chọn binary lúc chạy.
//! - **Windows Sandbox (Hyper-V)** — không có trên bản Home, và quan trọng hơn: nó không
//!   tác động được lên workspace thật của người dùng. Hợp cho computer-use agent, không
//!   hợp cho coding agent.
//! - **Mandatory Integrity Control (Low IL)** — để lại nhãn SACL trên đĩa, ảnh hưởng mọi
//!   tiến trình Low-integrity khác trên máy. Không tách được ranh giới của riêng agent.
//!
//! Nên mục Windows nằm ở v1.0 của lộ trình, và cho tới lúc đó câu trả lời trung thực là
//! câu trả lời trong tệp này.

use crate::policy::Policy;
use crate::seam::{Enforcement, SandboxError, SandboxProvider};

/// Lý do mặc định cho Windows, viết một lần để mọi chỗ nói cùng một câu.
pub const WINDOWS_REASON: &str = "Windows chưa có backend giam tiến trình: restricted \
     token là đường khả thi duy nhất và nó chưa được viết. Lệnh chạy với đầy đủ quyền \
     của bạn, và thứ duy nhất đứng giữa là hộp thoại duyệt.";

pub struct Unconfined {
    reason: String,
}

impl Unconfined {
    pub fn new(reason: impl Into<String>) -> Unconfined {
        Unconfined {
            reason: reason.into(),
        }
    }
}

impl SandboxProvider for Unconfined {
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError> {
        // `danger-full-access` không đòi hỏi gì, nên nó vẫn chạy được ở đây. Hai chế độ
        // kia thì **lỗi**, không phải chạy nguyên văn: trả lại argv gốc cho một người
        // gọi đã xin được giam là đúng cái hành vi làm cho một vòng vây không tồn tại
        // trông như đang có.
        if argv.is_empty() {
            return Err(SandboxError::EmptyArgv);
        }
        if policy.mode.confining() {
            return Err(SandboxError::Unavailable(self.reason.clone()));
        }
        Ok(argv)
    }

    fn enforcement(&self) -> Enforcement {
        Enforcement::None(self.reason.clone())
    }
}
