//! Seam terminal: từ vựng, và cái trait mà một bản cài đặt phải cung cấp.
//!
//! Mọi hàm nhận [`Owner`] làm tham số **đầu tiên**, kể cả những hàm mà một bản cài đặt
//! ngây thơ không cần tới nó. Đó là chủ ý: chủ sở hữu là một phần của câu hỏi ("phiên `x`
//! **của tôi**"), không phải một tầng lọc dán bên ngoài. Một seam để chủ ra ngoài chữ ký
//! là một seam mà bản cài đặt thứ hai sẽ quên kiểm, và cái quên đó không lộ ra cho tới lúc
//! có agent thứ hai.

use std::path::PathBuf;

use async_trait::async_trait;
use pai_core::{ScopeKey, ServiceKey};

/// Kích thước mặc định của một phiên mới.
///
/// 80×24 chứ không phải một con số to hơn: nó là mặc định mà mọi công cụ dòng lệnh được
/// thử nghiệm với, nên nó là kích thước mà cách xuống dòng của chúng ít bất ngờ nhất.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// Trần bộ đệm cho một phiên, tính bằng dòng.
///
/// Đủ để chứa một bản build thất bại trọn vẹn, không đủ để một máy chủ phát triển chạy qua
/// đêm ăn hết bộ nhớ.
pub const DEFAULT_MAX_LINES: usize = 5_000;

/// Chủ của một phiên.
///
/// Chính là phạm vi của `pai-core`: `None` là ngữ cảnh gốc — host, không phải agent — còn
/// `Some(scope)` là một agent cụ thể. Dùng lại kiểu của lõi chứ không đặt một danh tính
/// mới, vì hai hệ danh tính song song là hai hệ sẽ lệch nhau đúng vào lúc có agent lồng
/// agent.
pub type Owner = Option<ScopeKey>;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    /// Cùng một câu cho "không có" và cho "của người khác", cố ý — xem [`TerminalHost`].
    #[error("không có phiên terminal `{0}`")]
    NoSession(String),
    #[error("không có backend terminal `{0}`; đang có: {1}")]
    NoBackend(String, String),
    #[error("không mở được phiên: {0}")]
    Spawn(String),
    #[error("phiên `{0}` đã kết thúc")]
    Ended(String),
    #[error("không ghi được vào phiên: {0}")]
    Write(String),
    /// Có tín hiệu chỉ đúng khi nhắm vào một tiến trình khác shell của phiên.
    #[error("{0}")]
    Refused(String),
}

/// Tín hiệu mà một agent được phép gửi.
///
/// Một enum đóng chứ không phải một số: `kill -9` vào đúng cái shell của phiên bỏ lại một
/// cây tiến trình mồ côi và một bộ đệm không ai đọc nữa, và đó là loại việc phải đi qua
/// [`TerminalHost::close`] — nơi có phần chờ cho cây tiến trình biến mất.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Int,
    Term,
    Kill,
    Tstp,
    Hup,
}

impl Signal {
    pub fn parse(name: &str) -> Option<Signal> {
        match name {
            "SIGINT" => Some(Signal::Int),
            "SIGTERM" => Some(Signal::Term),
            "SIGKILL" => Some(Signal::Kill),
            "SIGTSTP" => Some(Signal::Tstp),
            "SIGHUP" => Some(Signal::Hup),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Signal::Int => "SIGINT",
            Signal::Term => "SIGTERM",
            Signal::Kill => "SIGKILL",
            Signal::Tstp => "SIGTSTP",
            Signal::Hup => "SIGHUP",
        }
    }
}

/// Yêu cầu mở một phiên.
pub struct OpenRequest {
    /// Backend đã đăng ký. Hiện chỉ có `shell`.
    pub backend: String,
    /// Tên hiển thị, cục bộ theo chủ. `None` thì lấy id rút gọn.
    pub name: Option<String>,
    /// `None` thì lấy gốc workspace của bản triển khai.
    pub cwd: Option<PathBuf>,
}

/// Ảnh chụp một phiên, đủ để giao diện và mô hình nói về nó.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub cwd: String,
    pub rows: u16,
    pub cols: u16,
    /// Shell của phiên còn sống không. Một phiên đã chết vẫn đọc được bộ đệm của nó cho
    /// tới khi ai đó đóng: chữ cuối cùng trước lúc chết thường là chữ đáng đọc nhất.
    pub alive: bool,
}

/// Cách chờ sau một lần gửi.
///
/// Không có "chờ tới khi lời nhắc hiện ra": nhận diện lời nhắc là so chuỗi với thứ mà
/// người dùng được phép đặt tuỳ ý trong `PS1`, và một REPL thì có lời nhắc của riêng nó.
/// Im lặng thì đúng cho cả hai — và khi nó sai, nó sai theo hướng trả về sớm một phần
/// output, chứ không theo hướng khẳng định một việc đã xong.
#[derive(Clone, Copy, Debug)]
pub struct Wait {
    /// Không có gì mới trong bấy lâu thì coi là đã yên.
    pub quiet: std::time::Duration,
    /// Trần tuyệt đối, để một lệnh không bao giờ im giữ mãi một lượt.
    pub timeout: std::time::Duration,
}

/// Vì sao lần chờ dừng lại. Đi thẳng vào chữ mà mô hình đọc: "chưa xong" và "xong" là hai
/// kết luận khác nhau, và đoán nhầm chiều nào cũng tốn một lượt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// Không chờ: gửi rồi trả về ngay.
    Background,
    Quiet,
    Timeout,
    Ended,
}

/// Kết quả một lần gửi.
pub struct Sent {
    pub lines: Vec<String>,
    /// Số dòng đã rơi khỏi bộ đệm, tính từ lúc mở phiên.
    pub dropped: usize,
    pub stopped: Stop,
}

/// Bản cài đặt của seam.
///
/// Một id không tồn tại và một id thuộc chủ khác trả về **cùng một lỗi**, cùng lý do như
/// `pai-tools::not_available`: hai câu trả lời khác nhau biến hàm tra cứu thành một máy dò,
/// và một agent kiên nhẫn sẽ liệt kê được phiên của agent bên cạnh mà không cần đọc được
/// dòng nào trong đó.
#[async_trait]
pub trait TerminalHost: Send + Sync + 'static {
    async fn open(&self, owner: Owner, req: OpenRequest) -> Result<SessionInfo, TerminalError>;

    fn list(&self, owner: Owner) -> Vec<SessionInfo>;

    fn info(&self, owner: Owner, id: &str) -> Result<SessionInfo, TerminalError>;

    /// Ghi byte vào PTY, rồi chờ hoặc không chờ.
    ///
    /// Người gọi tự quyết định có kèm `\n` hay không: một REPL đang chờ nốt vế sau của một
    /// biểu thức thì thêm `\n` là gửi đi một câu chưa xong.
    ///
    /// Phần **chờ** nằm ở đây chứ không ở tool, vì nó là câu hỏi "phiên này đã yên chưa" và
    /// chỉ bản cài đặt mới nhìn thấy dòng byte để trả lời. Một tool tự canh giờ rồi gọi
    /// `read` là một tool đoán, và hai tool sẽ đoán theo hai cách.
    async fn send(
        &self,
        owner: Owner,
        id: &str,
        bytes: &[u8],
        wait: Option<Wait>,
    ) -> Result<Sent, TerminalError>;

    /// Đọc một trang từ bộ đệm. `offset` đếm **từ dòng mới nhất về sau**.
    fn read(
        &self,
        owner: Owner,
        id: &str,
        offset: usize,
        count: usize,
    ) -> Result<crate::buffer::Page, TerminalError>;

    /// Đổi kích thước cửa sổ. Không có tool nào gọi nó — xem ghi chú ở [`crate::tools`] —
    /// nhưng nó nằm trên seam vì một giao diện gắn một phiên vào một khung co giãn được
    /// thì phải nói được điều đó với `SIGWINCH`, và không có đường nào khác để nói.
    fn resize(&self, owner: Owner, id: &str, rows: u16, cols: u16) -> Result<(), TerminalError>;

    /// Gửi tín hiệu cho nhóm tiến trình tiền cảnh của phiên.
    fn signal(&self, owner: Owner, id: &str, signal: Signal) -> Result<(), TerminalError>;

    /// Đóng một phiên và **chờ cây tiến trình của nó biến mất**.
    async fn close(&self, owner: Owner, id: &str) -> Result<(), TerminalError>;

    /// Đóng sạch, không hỏi chủ. Chỉ dành cho lúc gỡ plugin.
    async fn close_all(&self);
}

/// Seam.
pub enum Terminals {}

impl ServiceKey for Terminals {
    type Api = dyn TerminalHost;
    const NAME: &'static str = "terminals";
}
