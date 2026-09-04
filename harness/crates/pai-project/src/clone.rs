//! Fetch a repo with `git clone`, run as a child process so the user's credential helper
//! and ssh-agent work. The two traps handled here: password prompts hang a terminal-less
//! child (hence `GIT_TERMINAL_PROMPT=0`), and progress goes to stderr separated by `\r`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures::stream::BoxStream;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum CloneError {
    #[error("chưa có URL để clone")]
    Empty,
    /// See [`check_url`] — this is a command-execution hole, not a formatting matter.
    #[error(
        "từ chối `{0}::` — dạng URL này bảo `git` chạy một chương trình phụ trợ, tức là \
         chạy lệnh tuỳ ý trên máy bạn"
    )]
    TransportHelper(String),
    #[error("từ chối URL bắt đầu bằng `-`: `git` sẽ hiểu nó là một tuỳ chọn dòng lệnh")]
    LeadingDash,
    #[error(
        "từ chối scheme `{0}`: chỉ nhận https, http, ssh, git, file, hoặc dạng \
         `người-dùng@máy-chủ:đường-dẫn`"
    )]
    Scheme(String),
    #[error("không suy được tên thư mục từ URL `{0}` — hãy đặt tên tường minh")]
    NoName(String),
    #[error(
        "tên thư mục `{0}` không dùng được: không được rỗng, không được chứa `/`, `\\` hay `..`"
    )]
    BadName(String),
    #[error("{0} đã tồn tại và không rỗng — clone đè lên đó là mất dữ liệu")]
    DestinationNotEmpty(PathBuf),
    #[error("{0} đã là một tệp")]
    DestinationIsFile(PathBuf),
    #[error("không đọc được thư mục đích {0}: {1}")]
    DestinationUnreadable(PathBuf, String),
    #[error("không chạy được `git`: {0} — hãy kiểm tra `git` đã có trong PATH chưa")]
    Spawn(String),
}

/// A clone request: enough to build the command line, but **not yet validated**.
#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub url: String,
    /// The containing directory, not the destination: the destination is `parent/<name>`.
    pub parent: PathBuf,
    /// Left empty, it is derived from the URL.
    pub name: Option<String>,
    /// `Some(n)` is a shallow clone. Enough to read the code, not enough to read history.
    pub depth: Option<u32>,
}

/// What the UI sees while a clone runs; `Phase` only on change, `Line` only for non-progress output such as warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneEvent {
    Phase { label: String },
    Progress { label: String, percent: u8 },
    Line { text: String },
    Done { path: PathBuf },
    Failed { message: String },
}

impl CloneRequest {
    /// `parent` joined with the given name, or the name derived from the URL.
    pub fn destination(&self) -> Result<PathBuf, CloneError> {
        let ten = match self.name.as_deref() {
            Some(ten) => ten.to_string(),
            None => name_from_url(&self.url)?,
        };
        // Validate the derived name too: a URL ending in `/../` escapes `parent` just as a typed name would.
        check_name(&ten)?;
        Ok(self.parent.join(ten))
    }

    /// Everything has to be right **before** any child process is spawned.
    pub fn validate(&self) -> Result<(), CloneError> {
        check_url(&self.url)?;
        let dich = self.destination()?;
        check_destination(&dich)
    }
}

/// Blocks command execution: any `<x>::` transport helper (`ext::sh -c ...` runs a command) and any URL starting with `-`.
fn check_url(url: &str) -> Result<(), CloneError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CloneError::Empty);
    }
    if url.starts_with('-') {
        return Err(CloneError::LeadingDash);
    }
    if let Some(vi_tri) = url.find("::") {
        let dau = &url[..vi_tri];
        // `https://host/a::b` is not a helper: the `::` must come before the first slash, as `git` parses it.
        if !dau.contains('/') {
            return Err(CloneError::TransportHelper(dau.to_string()));
        }
    }

    if let Some((scheme, phan_con_lai)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git" | "file") {
            return Err(CloneError::Scheme(scheme.to_string()));
        }
        if phan_con_lai.is_empty() {
            return Err(CloneError::Scheme(scheme.to_string()));
        }
        return Ok(());
    }

    // The scp form `[user@]host:path`: no scheme, told apart by a colon with no slash before it.
    match url.split_once(':') {
        Some((truoc, sau)) if !truoc.is_empty() && !truoc.contains('/') && !sau.is_empty() => {
            Ok(())
        }
        // A bare local path lands here and is refused: `file://` states the intent, a schemeless string looks like a typo.
        _ => Err(CloneError::Scheme(url.to_string())),
    }
}

/// A directory name, not a path. This is what keeps the destination inside `parent`.
fn check_name(ten: &str) -> Result<(), CloneError> {
    let hong = ten.is_empty()
        || ten == "."
        || ten.contains('/')
        || ten.contains('\\')
        || ten.contains("..");
    if hong {
        return Err(CloneError::BadName(ten.to_string()));
    }
    Ok(())
}

fn name_from_url(url: &str) -> Result<String, CloneError> {
    let goc = url.trim().trim_end_matches('/');
    let cuoi = goc.rsplit(['/', ':']).next().unwrap_or("");
    let ten = cuoi.strip_suffix(".git").unwrap_or(cuoi);
    if ten.is_empty() {
        return Err(CloneError::NoName(url.to_string()));
    }
    Ok(ten.to_string())
}

/// Either does not exist, or is an empty directory. There is no third option.
fn check_destination(dich: &Path) -> Result<(), CloneError> {
    if dich.is_file() {
        return Err(CloneError::DestinationIsFile(dich.to_path_buf()));
    }
    match std::fs::read_dir(dich) {
        Ok(mut muc) => {
            if muc.next().is_some() {
                Err(CloneError::DestinationNotEmpty(dich.to_path_buf()))
            } else {
                Ok(())
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(CloneError::DestinationUnreadable(
            dich.to_path_buf(),
            err.to_string(),
        )),
    }
}

/// Progress stream of one clone; dropping it cancels the whole `git` process group. Needs a Tokio runtime.
pub fn clone(req: CloneRequest) -> BoxStream<'static, CloneEvent> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(run_clone(req, tx));
    Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
}

enum Ket {
    Xong(std::process::ExitStatus),
    Huy,
    Loi(String),
}

async fn run_clone(req: CloneRequest, tx: mpsc::Sender<CloneEvent>) {
    let dich = match req.validate().and_then(|()| req.destination()) {
        Ok(dich) => dich,
        Err(err) => {
            let _ = tx
                .send(CloneEvent::Failed {
                    message: err.to_string(),
                })
                .await;
            return;
        }
    };
    // `validate` just asserted the destination is empty or absent, so anything later came from git.
    let da_co_san = dich.is_dir();

    let mut lenh = Command::new("git");
    lenh.arg("clone").arg("--progress");
    if let Some(depth) = req.depth {
        lenh.arg("--depth").arg(depth.to_string());
    }
    // `--` before the URL: a second layer in case a later edit loosens `check_url`.
    lenh.arg("--").arg(req.url.trim()).arg(&dich);
    lenh.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Without these three, a private repo hangs forever on a password prompt aimed at a nonexistent terminal.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "");

    #[cfg(unix)]
    lenh.process_group(0);

    let mut con = match lenh.spawn() {
        Ok(con) => con,
        Err(err) => {
            let _ = tx
                .send(CloneEvent::Failed {
                    message: CloneError::Spawn(err.to_string()).to_string(),
                })
                .await;
            return;
        }
    };
    let Some(stderr) = con.stderr.take() else {
        let _ = tx
            .send(CloneEvent::Failed {
                message: CloneError::Spawn("không lấy được đầu ra của `git`".into()).to_string(),
            })
            .await;
        return;
    };

    // One tick immediately: DNS and auth can take seconds before git's first line.
    if tx
        .send(CloneEvent::Phase {
            label: "Đang tạo bản sao".to_string(),
        })
        .await
        .is_err()
    {
        kill_group(&mut con).await;
        return;
    }

    let mut cuoi = VecDeque::new();
    let ket = tokio::select! {
        // `closed()` catches a dropped stream while git waits on the network and sends nothing.
        _ = tx.closed() => Ket::Huy,
        ket = pump(&mut con, stderr, &tx, &mut cuoi) => ket,
    };

    match ket {
        Ket::Huy => {
            kill_group(&mut con).await;
            // Remove the partial download, or the next attempt trips the not-empty check.
            let _ = std::fs::remove_dir_all(&dich);
            if da_co_san {
                let _ = std::fs::create_dir_all(&dich);
            }
        }
        Ket::Loi(message) => {
            kill_group(&mut con).await;
            let _ = tx.send(CloneEvent::Failed { message }).await;
        }
        Ket::Xong(trang_thai) if trang_thai.success() => {
            let _ = tx.send(CloneEvent::Done { path: dich }).await;
        }
        Ket::Xong(trang_thai) => {
            let ma = trang_thai
                .code()
                .map(|ma| ma.to_string())
                .unwrap_or_else(|| "bị tín hiệu dừng".to_string());
            // Include git's last lines: "exit 128" alone cannot tell a typo from a missing permission.
            let chi_tiet: Vec<String> = cuoi.into_iter().collect();
            let message = if chi_tiet.is_empty() {
                format!("`git clone` thất bại ({ma})")
            } else {
                format!("`git clone` thất bại ({ma}): {}", chi_tiet.join(" / "))
            };
            let _ = tx.send(CloneEvent::Failed { message }).await;
        }
    }
}

/// Read stderr as bytes and turn it into events: git separates progress with `\r`, and a multi-byte char can straddle two reads.
async fn pump(
    con: &mut Child,
    mut stderr: ChildStderr,
    tx: &mpsc::Sender<CloneEvent>,
    cuoi: &mut VecDeque<String>,
) -> Ket {
    let mut mang = [0u8; 4096];
    let mut dem: Vec<u8> = Vec::new();
    let mut pha = String::new();

    loop {
        let so_byte = match stderr.read(&mut mang).await {
            Ok(0) => break,
            Ok(so_byte) => so_byte,
            Err(err) => return Ket::Loi(format!("không đọc được đầu ra của `git`: {err}")),
        };
        dem.extend_from_slice(&mang[..so_byte]);

        while let Some(vi_tri) = dem.iter().position(|byte| *byte == b'\r' || *byte == b'\n') {
            let doan: Vec<u8> = dem.drain(..=vi_tri).collect();
            let text = String::from_utf8_lossy(&doan[..vi_tri]).trim().to_string();
            if text.is_empty() {
                continue;
            }
            remember(cuoi, &text);
            for su_kien in translate_line(&text, &mut pha) {
                if tx.send(su_kien).await.is_err() {
                    return Ket::Huy;
                }
            }
        }
    }

    match con.wait().await {
        Ok(trang_thai) => Ket::Xong(trang_thai),
        Err(err) => Ket::Loi(format!("không chờ được `git`: {err}")),
    }
}

/// Keep only the last few lines: an error message needs context, not the whole log.
fn remember(cuoi: &mut VecDeque<String>, text: &str) {
    cuoi.push_back(text.to_string());
    if cuoi.len() > 5 {
        cuoi.pop_front();
    }
}

fn translate_line(text: &str, pha: &mut String) -> Vec<CloneEvent> {
    // Strip the server's `remote: ` prefix so the rest parses like a local line.
    let sach = text.strip_prefix("remote: ").unwrap_or(text).trim();
    match parse_progress(sach) {
        Some((goc, percent)) => {
            let label = phase_label(&goc);
            let mut ra = Vec::new();
            if *pha != label {
                pha.clear();
                pha.push_str(&label);
                ra.push(CloneEvent::Phase {
                    label: label.clone(),
                });
            }
            ra.push(CloneEvent::Progress { label, percent });
            ra
        }
        None => vec![CloneEvent::Line {
            text: sach.to_string(),
        }],
    }
}

/// `Receiving objects:  42% (100/240)` → `("Receiving objects", 42)`.
fn parse_progress(dong: &str) -> Option<(String, u8)> {
    let (dau, sau) = dong.split_once(':')?;
    let vi_tri = sau.find('%')?;
    let mut so: Vec<char> = sau[..vi_tri]
        .chars()
        .rev()
        .take_while(|ky_tu| ky_tu.is_ascii_digit())
        .collect();
    if so.is_empty() {
        return None;
    }
    so.reverse();
    let phan_tram: u32 = so.into_iter().collect::<String>().parse().ok()?;
    Some((dau.trim().to_string(), phan_tram.min(100) as u8))
}

/// Git's phase names in Vietnamese for the UI; an unknown phase is returned verbatim rather than dropped.
fn phase_label(goc: &str) -> String {
    match goc {
        "Counting objects" => "Đang đếm đối tượng",
        "Enumerating objects" => "Đang liệt kê đối tượng",
        "Compressing objects" => "Đang nén đối tượng",
        "Receiving objects" => "Đang nhận đối tượng",
        "Resolving deltas" => "Đang phân giải delta",
        "Updating files" => "Đang cập nhật tệp",
        "Checking out files" => "Đang lấy tệp ra",
        khac => return khac.to_string(),
    }
    .to_string()
}

/// SIGTERM then SIGKILL the whole group: killing only `git` leaves its helper children writing into the directory.
async fn kill_group(con: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = con.id() {
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
    let _ = con.kill().await;
}
