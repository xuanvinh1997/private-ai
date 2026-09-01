//! Một phiên PTY: cái shell, cái vòng đệm, và cây tiến trình đằng sau nó.
//!
//! Bài học đắt nhất của `pai-shell` được chép nguyên sang đây: **cái ta chạy là một cây
//! tiến trình, không phải một tiến trình.** Nhưng một phiên bền còn có thêm một cách để
//! làm sai mà một lần chạy `bash` không có — **job control**.
//!
//! Một shell tương tác bật `monitor mode`, và monitor mode đặt mỗi công việc chạy nền vào
//! một **nhóm tiến trình riêng**. Lúc đó `kill(-pgid_của_shell)` chỉ giết cái shell; cái
//! `npm run dev` vừa bấm `&` vẫn giữ cổng 3000 và vẫn ghi vào thư mục làm việc, còn bộ đệm
//! của nó thì không còn ai đọc. Trông thì mọi thứ vẫn chạy — đúng cái hình dạng của lỗi mà
//! `pai-shell` cảnh báo.
//!
//! Nên phiên được **mồi** bằng `set +m` ngay sau khi mở, trước khi ai kịp gửi gì vào. Từ
//! đó mọi thứ con cháu sinh ra nằm trong đúng một nhóm tiến trình, và một tín hiệu gửi cho
//! cả nhóm là đủ — cùng cơ chế, cùng dòng mã, cùng bảo đảm như `pai-shell`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::buffer::{Page, Ring};
use crate::seam::{Owner, SessionInfo, Signal, TerminalError};

/// Cho cái shell bao lâu để in xong lời nhắc đầu tiên trước khi ta xoá phần mồi đi.
///
/// Ngắn thì phần mồi lọt vào bộ đệm và mô hình đọc được một dòng nó không gõ; dài thì mỗi
/// lần mở phiên đứng lại chừng đó. Đây là một sự đánh đổi chứ không phải một hằng số đúng.
const PRIME_SETTLE: Duration = Duration::from_millis(250);

/// Chờ bao lâu cho cây tiến trình tự dọn trước khi ép.
const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// Nhịp hỏi lại "chết chưa".
const POLL: Duration = Duration::from_millis(25);

pub struct Session {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub cwd: PathBuf,
    pub owner: Owner,
    /// Nhóm tiến trình của phiên. Trên Unix, `portable-pty` gọi `setsid()` trong tiến
    /// trình con nên nó vừa là session leader vừa là group leader: pid chính là pgid.
    pid: Option<u32>,
    master: Mutex<Box<dyn MasterPty>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    size: Mutex<PtySize>,
    buffer: Arc<Mutex<Ring>>,
}

/// Mọi thứ cần biết để mở một phiên, gom thành một chỗ.
///
/// Một struct chứ không phải chín tham số: chín tham số cùng kiểu chuỗi là chín cơ hội để
/// hoán vị hai cái mà trình biên dịch không nói gì.
pub struct Spec {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub owner: Owner,
    pub cwd: PathBuf,
    pub max_lines: usize,
    pub rows: u16,
    pub cols: u16,
}

impl Session {
    /// Mở một phiên.
    ///
    /// `argv` đã đi qua vòng giam nếu có ai cắm vòng giam — việc bọc thuộc về
    /// [`crate::provider`], vì nó là chỗ giữ `Context` và chỉ nó mới hỏi được seam sandbox
    /// **tại thời điểm mở**.
    pub fn open(spec: Spec, argv: &[String]) -> Result<Arc<Session>, TerminalError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| TerminalError::Spawn("argv rỗng".into()))?;
        let cwd: &Path = &spec.cwd;

        let size = PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;

        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        // Một `TERM` có nghĩa là nửa còn lại của lời hứa "đây là terminal thật": không có
        // nó thì `isatty` trả về đúng nhưng mọi thư viện curses vẫn từ chối vẽ.
        command.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;
        // Thả đầu slave ngay: chừng nào ta còn giữ nó, đầu master không bao giờ thấy EOF,
        // và luồng bơm sẽ chờ mãi một mẩu byte không bao giờ tới sau khi con cháu đã chết.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| TerminalError::Spawn(err.to_string()))?;

        let buffer = Arc::new(Mutex::new(Ring::new(spec.max_lines)));
        {
            // Đọc PTY là một lời gọi chặn không huỷ được, nên nó thuộc về một luồng của hệ
            // điều hành chứ không phải một task: một `spawn_blocking` treo ở đây là một ô
            // vĩnh viễn bị chiếm trong bể luồng dùng chung của cả ứng dụng.
            let sink = buffer.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                while let Ok(read) = reader.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    sink.lock().push(&chunk[..read]);
                }
            });
        }

        let session = Arc::new(Session {
            id: spec.id,
            name: spec.name,
            backend: spec.backend,
            cwd: spec.cwd.clone(),
            owner: spec.owner,
            pid: child.process_id(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            size: Mutex::new(size),
            buffer,
        });
        Ok(session)
    }

    /// Tắt job control rồi xoá dấu vết của chính việc đó.
    ///
    /// Tách khỏi [`Session::open`] vì nó cần `await`, và vì lý do của nó nằm ở đầu module
    /// chứ không ở đây.
    pub async fn prime(&self) {
        #[cfg(unix)]
        {
            if self.write(b"set +m\n").is_err() {
                return;
            }
        }
        tokio::time::sleep(PRIME_SETTLE).await;
        // Bộ đệm bắt đầu từ đây. Một dòng mô hình không gõ ra là một dòng nó sẽ cố giải
        // thích.
        self.buffer.lock().reset();
    }

    pub fn info(&self) -> SessionInfo {
        let size = *self.size.lock();
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            backend: self.backend.clone(),
            cwd: self.cwd.display().to_string(),
            rows: size.rows,
            cols: size.cols,
            alive: self.alive(),
        }
    }

    pub fn alive(&self) -> bool {
        matches!(self.child.lock().try_wait(), Ok(None))
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), TerminalError> {
        let mut writer = self.writer.lock();
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|err| TerminalError::Write(err.to_string()))
    }

    pub fn read(&self, offset: usize, count: usize) -> Page {
        self.buffer.lock().page(offset, count)
    }

    /// Mốc để hỏi "có gì mới kể từ đây".
    pub fn mark(&self) -> u64 {
        self.buffer.lock().produced()
    }

    pub fn since(&self, mark: u64) -> Vec<String> {
        self.buffer.lock().since(mark)
    }

    pub fn dropped(&self) -> usize {
        self.buffer.lock().dropped()
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), TerminalError> {
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .lock()
            .resize(size)
            .map_err(|err| TerminalError::Write(err.to_string()))?;
        *self.size.lock() = size;
        Ok(())
    }

    /// Nhóm tiến trình đang ở tiền cảnh, và nó có phải chính cái shell của phiên không.
    ///
    /// Câu hỏi thứ hai mới là câu quan trọng: `SIGKILL` vào đúng cái shell bỏ lại một cây
    /// tiến trình mồ côi, nên nó bị từ chối ở [`Session::signal`].
    #[cfg(unix)]
    fn foreground(&self) -> (Option<u32>, bool) {
        let leader = self
            .master
            .lock()
            .process_group_leader()
            .map(|pid| pid as u32);
        match (leader, self.pid) {
            (Some(leader), Some(shell)) => (Some(leader), leader == shell),
            (None, shell) => (shell, true),
            (leader, None) => (leader, false),
        }
    }

    #[cfg(unix)]
    pub fn signal(&self, signal: Signal) -> Result<(), TerminalError> {
        let (target, is_shell) = self.foreground();
        if signal == Signal::Kill && is_shell {
            return Err(TerminalError::Refused(
                "SIGKILL nhắm vào chính shell của phiên bỏ lại một cây tiến trình không ai \
                 dọn; dùng `terminal_close`, nó có phần chờ cho cây tiến trình biến mất."
                    .into(),
            ));
        }
        let Some(target) = target else {
            return Err(TerminalError::Ended(self.id.clone()));
        };
        signal_group(target, raw_signal(signal));
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn signal(&self, _signal: Signal) -> Result<(), TerminalError> {
        Err(TerminalError::Refused(
            "gửi tín hiệu chưa làm được trên nền tảng này".into(),
        ))
    }

    /// Đóng phiên và chờ cây tiến trình biến mất.
    ///
    /// Tín hiệu gửi cho **cả nhóm**, đúng như `pai-shell`: giết cái shell mà bỏ lại con
    /// cháu là bỏ lại những tiến trình giữ cổng và giữ khoá tệp mà không ai còn nhớ để dọn.
    pub async fn close(&self) {
        self.signal_all(sigterm());
        if self.wait_gone(CLOSE_GRACE).await {
            return;
        }
        self.signal_all(sigkill());
        // Cây tiến trình đã nhận `SIGKILL` thì phần chờ này chỉ còn là việc gặt xác con;
        // không đặt hạn giờ ở đây sẽ treo mãi nếu một tiến trình nằm trong trạng thái
        // không giết được, nên vẫn có hạn.
        self.wait_gone(CLOSE_GRACE).await;
        let _ = self.child.lock().kill();
    }

    fn signal_all(&self, signal: i32) {
        if let Some(pid) = self.pid {
            signal_group(pid, signal);
        }
    }

    /// `true` nếu tiến trình đã thoát trong khoảng chờ.
    async fn wait_gone(&self, budget: Duration) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if !matches!(self.child.lock().try_wait(), Ok(None)) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

/// Gửi tín hiệu cho cả nhóm tiến trình. Số âm nghĩa là "cả nhóm" — đây là toàn bộ lý do
/// phần mồi `set +m` tồn tại.
#[cfg(unix)]
fn signal_group(pid: u32, signal: i32) {
    unsafe { libc::kill(-(pid as i32), signal) };
}

#[cfg(not(unix))]
fn signal_group(_pid: u32, _signal: i32) {
    // Windows không có nhóm tiến trình theo nghĩa này; việc tương ứng là Job Object và nó
    // thuộc về `pai-sandbox`. Cùng lời hẹn, cùng chỗ, như `pai-shell`.
}

#[cfg(unix)]
fn raw_signal(signal: Signal) -> i32 {
    match signal {
        Signal::Int => libc::SIGINT,
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
        Signal::Tstp => libc::SIGTSTP,
        Signal::Hup => libc::SIGHUP,
    }
}

#[cfg(unix)]
fn sigterm() -> i32 {
    libc::SIGTERM
}
#[cfg(unix)]
fn sigkill() -> i32 {
    libc::SIGKILL
}
#[cfg(not(unix))]
fn sigterm() -> i32 {
    0
}
#[cfg(not(unix))]
fn sigkill() -> i32 {
    0
}
