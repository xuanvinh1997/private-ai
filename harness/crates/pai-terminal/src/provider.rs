//! Sổ phiên cục bộ: bản cài đặt của seam trên chính máy này.
//!
//! Hai ý đáng nhớ trong tệp này.
//!
//! **Phiên thuộc về ai tạo ra nó.** Mọi lời gọi trình một [`Owner`], và một phiên chỉ trả
//! lời đúng chủ của nó. Một id thuộc chủ khác nhận **cùng một lỗi** với một id không tồn
//! tại — hai câu trả lời khác nhau ở đây biến hàm tra cứu thành một máy dò, và một agent
//! kiên nhẫn sẽ đếm được số phiên của agent bên cạnh mà không đọc được dòng nào trong đó.
//!
//! **Vòng giam hỏi lúc mở, không hỏi lúc dựng.** Provider giữ một `Context` chứ không giữ
//! một `Arc<dyn SandboxProvider>`, đúng lý do như `pai-shell::provider`: gỡ plugin sandbox
//! ra phải làm mọi phiên mở sau đó chạy không giam, chứ không phải đi qua một bản sao còn
//! sót lại. Và một vòng bọc **hỏng** thì không mở phiên — một lần bỏ qua im lặng là một
//! lần người dùng tin vào một vòng vây không tồn tại.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use pai_core::Context;
use pai_sandbox::{Policy, Sandbox};
use parking_lot::Mutex;

use crate::buffer::Page;
use crate::seam::{
    DEFAULT_COLS, DEFAULT_MAX_LINES, DEFAULT_ROWS, OpenRequest, Owner, Sent, SessionInfo, Signal,
    Stop, TerminalError, TerminalHost, Wait,
};
use crate::session::{Session, Spec};

/// Nhịp hỏi lại "có gì mới không" trong lúc chờ một lệnh yên.
///
/// Đủ nhặt để một lệnh nhanh không phải trả giá bằng một nhịp thừa, đủ thưa để việc chờ
/// không tự nó thành một vòng lặp bận.
const SETTLE_POLL: std::time::Duration = std::time::Duration::from_millis(25);

/// Backend duy nhất hiện có.
///
/// Là một chuỗi tra trong bảng chứ không phải một tham số `command`, vì `command` biến
/// `terminal_open` thành `bash` có trạng thái: mô hình gõ ra chương trình nào nó muốn và
/// cái duyệt của `terminal_open` không còn nói được gì cụ thể. Bảng đóng thì câu hỏi
/// "cho mở terminal không" có đúng một nghĩa.
pub const SHELL_BACKEND: &str = "shell";

pub struct LocalTerminals {
    ctx: Context,
    policy: Policy,
    cwd: PathBuf,
    max_lines: usize,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl LocalTerminals {
    pub fn new(ctx: Context, policy: Policy, cwd: PathBuf) -> LocalTerminals {
        LocalTerminals {
            ctx,
            policy,
            cwd,
            max_lines: DEFAULT_MAX_LINES,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_max_lines(mut self, lines: usize) -> LocalTerminals {
        self.max_lines = lines;
        self
    }

    /// argv sau khi đã bọc, nếu có ai bọc. Xem ghi chú đầu module.
    fn argv(&self) -> Result<Vec<String>, TerminalError> {
        // `-i` chứ không phải một lệnh: cái ta mở là một phiên, và một shell tương tác là
        // thứ duy nhất trả lời đúng cho `cd` ở lần gọi trước.
        let argv = vec!["/bin/sh".to_string(), "-i".to_string()];
        match self.ctx.get::<Sandbox>() {
            Some(sandbox) => sandbox
                .wrap(argv, &self.policy)
                .map_err(|err| TerminalError::Spawn(err.to_string())),
            None => Ok(argv),
        }
    }

    /// Tra một phiên **của đúng chủ này**.
    fn find(&self, owner: Owner, id: &str) -> Result<Arc<Session>, TerminalError> {
        let found = self.sessions.lock().get(id).cloned();
        match found {
            Some(session) if session.owner == owner => Ok(session),
            // Không tách hai nhánh còn lại. Xem ghi chú đầu module.
            _ => Err(TerminalError::NoSession(id.to_string())),
        }
    }
}

#[async_trait]
impl TerminalHost for LocalTerminals {
    async fn open(&self, owner: Owner, req: OpenRequest) -> Result<SessionInfo, TerminalError> {
        if req.backend != SHELL_BACKEND {
            return Err(TerminalError::NoBackend(
                req.backend,
                SHELL_BACKEND.to_string(),
            ));
        }

        let id = uuid::Uuid::now_v7().to_string();
        let name = req.name.unwrap_or_else(|| {
            // Một cái tên luôn có, kể cả khi không ai đặt: danh sách phiên là thứ người
            // dùng đọc, và một cột trống ở đó không nói được gì.
            format!("{}-{}", SHELL_BACKEND, &id[..8.min(id.len())])
        });
        let cwd = req.cwd.unwrap_or_else(|| self.cwd.clone());

        let session = Session::open(
            Spec {
                id: id.clone(),
                name,
                backend: SHELL_BACKEND.to_string(),
                owner,
                cwd,
                max_lines: self.max_lines,
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
            },
            &self.argv()?,
        )?;
        session.prime().await;

        let info = session.info();
        self.sessions.lock().insert(id, session);
        Ok(info)
    }

    fn list(&self, owner: Owner) -> Vec<SessionInfo> {
        let mut rows: Vec<SessionInfo> = self
            .sessions
            .lock()
            .values()
            .filter(|session| session.owner == owner)
            .map(|session| session.info())
            .collect();
        // Id là UUIDv7 nên sắp theo id là sắp theo thứ tự mở. Danh sách ổn định là thứ
        // khiến "cái thứ hai" có nghĩa qua hai lần gọi.
        rows.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    fn info(&self, owner: Owner, id: &str) -> Result<SessionInfo, TerminalError> {
        Ok(self.find(owner, id)?.info())
    }

    async fn send(
        &self,
        owner: Owner,
        id: &str,
        bytes: &[u8],
        wait: Option<Wait>,
    ) -> Result<Sent, TerminalError> {
        let session = self.find(owner, id)?;
        if !session.alive() {
            return Err(TerminalError::Ended(id.to_string()));
        }

        // Lấy mốc **trước** khi ghi: lấy sau thì phần output in ra giữa hai câu lệnh này
        // biến mất, và nó biến mất đúng vào những lệnh chạy nhanh nhất.
        let mark = session.mark();
        session.write(bytes)?;

        let Some(wait) = wait else {
            return Ok(Sent {
                lines: Vec::new(),
                dropped: session.dropped(),
                stopped: Stop::Background,
            });
        };

        let started = std::time::Instant::now();
        let mut last_seen = session.mark();
        let mut last_change = started;
        let stopped = loop {
            tokio::time::sleep(SETTLE_POLL).await;
            let now = session.mark();
            if now != last_seen {
                last_seen = now;
                last_change = std::time::Instant::now();
            }
            if !session.alive() {
                break Stop::Ended;
            }
            if last_change.elapsed() >= wait.quiet {
                break Stop::Quiet;
            }
            if started.elapsed() >= wait.timeout {
                break Stop::Timeout;
            }
        };

        Ok(Sent {
            lines: session.since(mark),
            dropped: session.dropped(),
            stopped,
        })
    }

    fn read(
        &self,
        owner: Owner,
        id: &str,
        offset: usize,
        count: usize,
    ) -> Result<Page, TerminalError> {
        // Đọc được cả phiên đã chết, cố ý: chữ cuối cùng trước lúc chết thường là chữ
        // đáng đọc nhất, và bắt đóng phiên mới đọc được nó là bắt vứt nó đi.
        Ok(self.find(owner, id)?.read(offset, count))
    }

    fn resize(&self, owner: Owner, id: &str, rows: u16, cols: u16) -> Result<(), TerminalError> {
        self.find(owner, id)?.resize(rows, cols)
    }

    fn signal(&self, owner: Owner, id: &str, signal: Signal) -> Result<(), TerminalError> {
        self.find(owner, id)?.signal(signal)
    }

    async fn close(&self, owner: Owner, id: &str) -> Result<(), TerminalError> {
        let session = self.find(owner, id)?;
        session.close().await;
        // Gỡ khỏi sổ **sau** khi cây tiến trình đã biến mất: gỡ trước thì một lần đóng
        // chậm trả lại một id "không tồn tại" trong khi tiến trình vẫn đang chạy.
        self.sessions.lock().remove(id);
        Ok(())
    }

    async fn close_all(&self) {
        let all: Vec<Arc<Session>> = self.sessions.lock().drain().map(|(_, s)| s).collect();
        for session in all {
            session.close().await;
        }
    }
}
