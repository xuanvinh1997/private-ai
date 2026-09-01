//! Seam giam tiến trình.
//!
//! Giao diện chỉ có hai hàm, và hàm thứ hai mới là hàm quan trọng.
//!
//! [`SandboxProvider::wrap`] bọc argv: nhận argv **thật** sắp được spawn (không phải một
//! chuỗi shell) và trả về argv mới. Người gọi chạy cái trả về, không chạy cái đã đưa
//! vào. Đây là hình dạng duy nhất hoạt động cho cả ba hệ điều hành, vì cả ba đều giam
//! bằng cách cho một tiến trình tự trói mình rồi `exec` — không có API nào giam được một
//! tiến trình đã chạy.
//!
//! [`SandboxProvider::enforcement`] trả lời "trên **máy đang chạy**, chuyện giam có thật
//! không". Nó không hỏi chính sách, vì câu hỏi không thuộc về chính sách: một chế độ
//! `workspace-write` trên máy không có Landlock vẫn là `workspace-write`, chỉ có điều
//! không ai thi hành nó. Trường này là thứ hộp thoại duyệt phải đọc, và là lý do
//! [`Enforcement`] có ba trạng thái chứ không phải hai.

use std::sync::Arc;

use pai_core::ServiceKey;

use crate::policy::Policy;

/// Chuyện giam có thật đến đâu, trên máy này, ngay lúc này.
///
/// Ba trạng thái chứ không phải `bool`, vì `Partial` là trạng thái hay gặp nhất trong
/// thực tế và cũng là trạng thái dễ bị làm tròn nhất — làm tròn lên thành "có giam" là
/// nói dối, làm tròn xuống thành "không giam" là vứt đi một lớp phòng thủ thật.
///
/// Lý do đi kèm `Partial` và `None` không phải để ghi log cho đẹp: nó là câu người dùng
/// sẽ đọc trong hộp thoại duyệt, ngay trước lúc bấm "cho phép".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enforcement {
    /// Kernel thi hành đúng cái đã khai. Ghi ngoài vùng cho phép là thất bại, không phải
    /// là "thường thì thất bại".
    Full,
    /// Có vòng vây, nhưng thủng ở chỗ đã biết. Người gọi cần một ranh giới tuyệt đối thì
    /// phải từ chối chạy hoặc nói ra, chứ không được coi như `Full`.
    Partial(String),
    /// Không có gì cả. Lệnh sẽ chạy với đầy đủ quyền của người dùng.
    None(String),
}

impl Enforcement {
    /// Có vòng vây nào không — kể cả vòng vây thủng.
    pub fn confines(&self) -> bool {
        !matches!(self, Enforcement::None(_))
    }

    /// Vòng vây có kín không. Dùng cho những chỗ cần một ranh giới tuyệt đối.
    pub fn is_full(&self) -> bool {
        matches!(self, Enforcement::Full)
    }

    /// Vì sao không kín. `None` khi kín.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Enforcement::Full => Option::None,
            Enforcement::Partial(reason) | Enforcement::None(reason) => Some(reason.as_str()),
        }
    }

    /// Một từ cho log và cho giao diện.
    pub fn label(&self) -> &'static str {
        match self {
            Enforcement::Full => "full",
            Enforcement::Partial(_) => "partial",
            Enforcement::None(_) => "none",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Không giam được, nên **không chạy**. Người gọi phải nói ra chứ tuyệt đối không
    /// được lặng lẽ chạy argv gốc: một lần bỏ qua im lặng là một lần người dùng tin vào
    /// một vòng vây không tồn tại.
    #[error("không giam được tiến trình trên máy này: {0}")]
    Unavailable(String),
    /// argv rỗng. Không phải lỗi của sandbox, nhưng bọc một argv rỗng thì tạo ra một
    /// dòng lệnh chạy được mà không ai định chạy.
    #[error("argv rỗng: không có gì để giam")]
    EmptyArgv,
    /// Không phân giải được một gốc trong chính sách.
    #[error("không phân giải được {0}: {1}")]
    Unresolvable(std::path::PathBuf, String),
}

/// Bản cài đặt của seam.
///
/// `wrap` nhận `Vec<String>` chứ không phải `&[String]` vì trong hai trong ba trường hợp
/// nó chỉ nối thêm phần đầu vào chính argv ấy — nhận theo sở hữu thì không có bản sao
/// nào bị tạo ra chỉ để bị vứt đi.
pub trait SandboxProvider: Send + Sync + 'static {
    /// Bọc argv để tiến trình chạy trong vòng giam. Trả về argv mới.
    ///
    /// Với `danger-full-access`, bản cài đặt trả lại **đúng** argv đã nhận: chế độ đó là
    /// sự vắng mặt của sandbox, nên bọc nó là dựng một vòng vây rỗng rồi phải nuôi.
    fn wrap(&self, argv: Vec<String>, policy: &Policy) -> Result<Vec<String>, SandboxError>;

    /// Chế độ này có thật sự được thi hành trên máy đang chạy không.
    fn enforcement(&self) -> Enforcement;
}

/// Seam. Đúng một provider cho mỗi cõi, chọn theo hệ điều hành lúc cắm plugin.
pub enum Sandbox {}

impl ServiceKey for Sandbox {
    type Api = dyn SandboxProvider;
    const NAME: &'static str = "sandbox";
}

/// Chọn provider cho máy đang chạy.
///
/// Chọn theo hệ điều hành **trước**, dò khả năng **sau**. Thứ tự ngược lại nghe có vẻ
/// tổng quát hơn nhưng nó dò cả những backend không bao giờ có mặt, và mỗi lần dò là một
/// lần spawn tiến trình lúc khởi động.
///
/// Ba bản, mỗi bản một hệ điều hành, thay vì một hàm với ba nhánh `cfg`: nhánh `cfg`
/// bên trong thân hàm là thứ chỉ được biên dịch ở đúng một nơi và mục ruỗng ở hai nơi kia.
#[cfg(target_os = "macos")]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    match crate::seatbelt::Seatbelt::detect() {
        Some(seatbelt) => Arc::new(seatbelt),
        Option::None => Arc::new(crate::Unconfined::new(
            "không tìm thấy /usr/bin/sandbox-exec trên máy này",
        )),
    }
}

/// Xem bản macOS.
#[cfg(target_os = "linux")]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    Arc::new(crate::landlock::Landlock::detect())
}

/// Xem bản macOS.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn for_this_machine() -> Arc<dyn SandboxProvider> {
    Arc::new(crate::Unconfined::new(crate::unconfined::WINDOWS_REASON))
}
