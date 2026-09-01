//! Mở một ống tới một language server, và cách tìm xem nó có trên máy hay không.
//!
//! Tách khỏi [`crate::client`] vì đúng lý do mà `pai-mcp` tách [`Dialer`] khỏi hub: phần
//! nói giao thức giống hệt nhau cho mọi cách mở ống, và bài kiểm chứng cần một cách mở
//! ống **không** đẻ tiến trình con — nếu không thì mọi bài kiểm về bắt tay, về server chết
//! giữa chừng và về hết giờ đều phải có `rust-analyzer` cài sẵn trên máy chạy CI.
//!
//! [`Dialer`]: pai_mcp

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::Child;

/// Một ống hai chiều tới một server đang chạy.
pub struct Channel {
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    /// Chỉ có với server là tiến trình con. Giữ lại để giết nó nếu `exit` không đủ —
    /// một server phớt lờ `exit` mà ta chỉ đóng ống thì nó thành tiến trình mồ côi chạy
    /// tới hết phiên làm việc của người dùng.
    pub(crate) child: Option<Child>,
}

impl Channel {
    pub fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Channel {
        Channel {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: None,
        }
    }
}

/// Cách mở một ống. Một lần gọi = một server mới.
#[async_trait]
pub trait Launch: Send + Sync + 'static {
    async fn launch(&self) -> anyhow::Result<Channel>;
    /// Tên để nói trong thông báo lỗi — cái người dùng nhận ra, không phải cái ta gõ.
    fn label(&self) -> String;
}

/// Một tiến trình con nói LSP trên stdin/stdout.
pub struct ChildLaunch {
    label: String,
    command: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

impl ChildLaunch {
    pub fn new(
        label: impl Into<String>,
        command: impl Into<PathBuf>,
        args: Vec<String>,
        cwd: impl Into<PathBuf>,
    ) -> ChildLaunch {
        ChildLaunch {
            label: label.into(),
            command: command.into(),
            args,
            cwd: cwd.into(),
        }
    }
}

#[async_trait]
impl Launch for ChildLaunch {
    async fn launch(&self) -> anyhow::Result<Channel> {
        let mut command = tokio::process::Command::new(&self.command);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr đi thẳng ra stderr của ta, giống `pai-mcp`: một server không khởi
            // động được gần như luôn nói lý do ở đó, và nuốt nó là biến một lỗi cấu hình
            // thành một bí ẩn.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("không lấy được stdin của `{}`", self.label))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("không lấy được stdout của `{}`", self.label))?;

        Ok(Channel {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            child: Some(child),
        })
    }

    fn label(&self) -> String {
        self.label.clone()
    }
}

/// Lệnh này có thật trên máy không, và ở đâu.
///
/// Dò ở đây, một lần, lúc cắm plugin — **không** dò lại mỗi lần gọi. Một tool có trong
/// danh sách mà lần nào gọi cũng lỗi là một tool dạy mô hình bỏ qua danh sách, và cái giá
/// của việc dò lại là một lượt quét `PATH` cho mỗi câu hỏi để trả lời một điều gần như
/// không bao giờ đổi giữa hai câu hỏi.
pub fn locate(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    // Có dấu ngăn thư mục nghĩa là người dùng đã chỉ đích danh; `PATH` không liên quan.
    if command.contains(std::path::MAIN_SEPARATOR) || command.contains('/') {
        let path = PathBuf::from(command);
        return runnable(&path).then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(command))
        .find(|candidate| runnable(candidate))
}

fn runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    // Trên Unix, một tệp không có bit `x` nằm trong `PATH` là chuyện thường (tệp dữ liệu,
    // tệp README); coi nó là lệnh thì ta đăng ký một tool rồi hỏng ngay lần gọi đầu.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
