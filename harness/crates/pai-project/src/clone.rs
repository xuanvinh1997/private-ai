//! Lấy một repo về bằng `git clone`.
//!
//! **Dùng `git` làm tiến trình con, không dùng libgit2/gix.** Một thư viện Rust clone
//! được, nhưng nó clone được đúng những repo công khai. Repo riêng tư thì cần credential
//! helper của người dùng — Keychain trên macOS, `git-credential-manager` trên Windows —
//! hoặc ssh-agent với khoá đã nạp sẵn, và toàn bộ bộ máy đó là của `git`, cấu hình trong
//! `~/.gitconfig` của người ta, không phải thứ tái tạo lại được trong tiến trình này. Nối
//! lại nó nghĩa là hỏi mật khẩu một lần nữa cho một thứ máy người ta đã đăng nhập rồi.
//!
//! Cái giá phải trả là hai cái bẫy, và cả hai đều nằm ở đây:
//!
//! 1. `git` **hỏi mật khẩu**. Trong một tiến trình con không có terminal, câu hỏi đó
//!    không hiện ra ở đâu cả và tiến trình treo vô hạn trong khi giao diện chỉ thấy im
//!    lặng. Nên `GIT_TERMINAL_PROMPT=0` cùng hai biến `*_ASKPASS` rỗng: thà thất bại
//!    ngay với "cần xác thực" còn hơn treo mãi mãi.
//! 2. `git` ghi tiến trình ra **stderr**, và ghi đè một dòng bằng `\r` chứ không xuống
//!    dòng. Tách theo `\n` thì cả bản clone là **một** dòng khổng lồ, tới đúng lúc mọi
//!    thứ đã xong — tức là thanh tiến trình đứng im rồi nhảy thẳng lên 100%.

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
    /// Xem [`kiem_url`] — đây là lỗ thi hành lệnh, không phải chuyện định dạng.
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

/// Một yêu cầu clone, đã đủ để dựng dòng lệnh nhưng **chưa được kiểm**.
#[derive(Debug, Clone)]
pub struct CloneRequest {
    pub url: String,
    /// Thư mục chứa, không phải thư mục đích: đích là `parent/<tên>`.
    pub parent: PathBuf,
    /// Bỏ trống thì suy từ URL.
    pub name: Option<String>,
    /// `Some(n)` là clone nông. Đủ để đọc mã, không đủ để xem lịch sử.
    pub depth: Option<u32>,
}

/// Những gì giao diện thấy trong lúc clone chạy.
///
/// `Phase` chỉ phát khi pha **đổi**, còn `Progress` phát theo từng nhịp git báo. `Line`
/// dành cho những dòng không phải tiến trình — cảnh báo, lỗi xác thực, `Cloning into` —
/// tức đúng những dòng cần đọc khi có sự cố. Phát cả dòng thô cho mỗi nhịp tiến trình
/// nữa thì khung chi tiết có vài trăm dòng giống hệt nhau và không còn đọc được.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneEvent {
    Phase { label: String },
    Progress { label: String, percent: u8 },
    Line { text: String },
    Done { path: PathBuf },
    Failed { message: String },
}

impl CloneRequest {
    /// `parent` ghép với tên đã đặt, hoặc tên suy từ URL.
    pub fn destination(&self) -> Result<PathBuf, CloneError> {
        let ten = match self.name.as_deref() {
            Some(ten) => ten.to_string(),
            None => ten_tu_url(&self.url)?,
        };
        // Kiểm cả tên suy ra chứ không chỉ tên người dùng gõ: một URL kết thúc bằng
        // `/../` cũng đẩy thư mục đích ra ngoài `parent` y như một cái tên gõ tay.
        kiem_ten(&ten)?;
        Ok(self.parent.join(ten))
    }

    /// Mọi thứ phải đúng **trước** khi có tiến trình con nào được sinh ra.
    pub fn validate(&self) -> Result<(), CloneError> {
        kiem_url(&self.url)?;
        let dich = self.destination()?;
        kiem_dich(&dich)
    }
}

/// Ba câu chặn, và câu đầu là câu quan trọng nhất trong tệp này.
///
/// **`ext::` là một lỗ thi hành lệnh.** `git clone "ext::sh -c 'lệnh bất kỳ'"` không tải
/// gì cả: nó bảo `git` chạy `sh -c 'lệnh bất kỳ'` làm chương trình vận chuyển. Một URL
/// dán từ chỗ khác vào ô "clone" vì thế là một dòng lệnh, và nó chạy với đúng quyền của
/// người dùng. Chặn **mọi** `<gì đó>::` chứ không chỉ `ext::` — danh sách helper mở rộng
/// được, và một danh sách cấm luôn đi sau.
///
/// Câu thứ hai: URL bắt đầu bằng `-` bị `git` nuốt làm cờ (`--upload-pack=...` chẳng hạn,
/// lại là thi hành lệnh). Dòng lệnh dưới kia luôn có `--` trước URL, nên đây là lớp thứ
/// hai — hai lớp vì lớp thứ nhất là thứ dễ bị xoá đi trong một lần sửa vội.
fn kiem_url(url: &str) -> Result<(), CloneError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CloneError::Empty);
    }
    if url.starts_with('-') {
        return Err(CloneError::LeadingDash);
    }
    if let Some(vi_tri) = url.find("::") {
        let dau = &url[..vi_tri];
        // `https://máy/a::b` không phải helper — dấu `::` nằm trong đường dẫn. Helper là
        // dạng có `::` **trước** dấu gạch chéo đầu tiên, đúng như `git` phân tích.
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

    // Dạng scp: `[người-dùng@]máy-chủ:đường-dẫn`. Không có scheme, phân biệt với đường
    // dẫn thường ở chỗ có dấu hai chấm mà trước nó không có gạch chéo.
    match url.split_once(':') {
        Some((truoc, sau)) if !truoc.is_empty() && !truoc.contains('/') && !sau.is_empty() => {
            Ok(())
        }
        // Đường dẫn cục bộ trần (`/nha/repo`) rơi vào đây. Từ chối có chủ ý: `file:///nha/repo`
        // nói ra ý định, còn một chuỗi không có scheme thì không phân biệt được với một
        // cái tên gõ nhầm.
        _ => Err(CloneError::Scheme(url.to_string())),
    }
}

/// Tên thư mục, không phải đường dẫn. Đây là chỗ giữ thư mục đích nằm trong `parent`.
fn kiem_ten(ten: &str) -> Result<(), CloneError> {
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

fn ten_tu_url(url: &str) -> Result<String, CloneError> {
    let goc = url.trim().trim_end_matches('/');
    let cuoi = goc.rsplit(['/', ':']).next().unwrap_or("");
    let ten = cuoi.strip_suffix(".git").unwrap_or(cuoi);
    if ten.is_empty() {
        return Err(CloneError::NoName(url.to_string()));
    }
    Ok(ten.to_string())
}

/// Chưa tồn tại, hoặc là một thư mục rỗng. Không có lựa chọn thứ ba.
fn kiem_dich(dich: &Path) -> Result<(), CloneError> {
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

/// Luồng tiến trình của một bản clone. **Thả luồng đi là huỷ bản clone.**
///
/// Huỷ ở đây không phải chuyện lịch sự: một `git clone` bỏ dở giữ nguyên một cây tiến
/// trình (`git` sinh `git-remote-https`, sinh `ssh`) vẫn đang tải, vẫn đang ghi vào thư
/// mục đích. Nên tiến trình con nằm trong nhóm tiến trình riêng và tín hiệu gửi cho cả
/// nhóm — cùng lý do và cùng cách làm với `pai-shell`.
///
/// Phải gọi trong một runtime Tokio: công việc thật chạy trên một task nền, và task đó
/// biết luồng đã bị thả nhờ đầu gửi của kênh đóng lại.
pub fn clone(req: CloneRequest) -> BoxStream<'static, CloneEvent> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(chay(req, tx));
    Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
}

enum Ket {
    Xong(std::process::ExitStatus),
    Huy,
    Loi(String),
}

async fn chay(req: CloneRequest, tx: mpsc::Sender<CloneEvent>) {
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
    // Nhớ lại trạng thái trước khi chạy để lúc huỷ biết được cái gì là của git. `validate`
    // vừa khẳng định chỗ này rỗng hoặc chưa có, nên mọi thứ xuất hiện sau đây là do git.
    let da_co_san = dich.is_dir();

    let mut lenh = Command::new("git");
    lenh.arg("clone").arg("--progress");
    if let Some(depth) = req.depth {
        lenh.arg("--depth").arg(depth.to_string());
    }
    // `--` trước URL: lớp thứ hai sau `kiem_url`, cho trường hợp một lần sửa sau này nới
    // lỏng câu chặn kia mà quên chỗ này.
    lenh.arg("--").arg(req.url.trim()).arg(&dich);
    lenh.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Không có ba biến này thì một repo riêng tư làm tiến trình con treo vô hạn: git
        // mở hộp thoại hỏi mật khẩu vào một terminal không tồn tại, và giao diện chỉ thấy
        // một luồng im lặng không bao giờ kết thúc.
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

    // Một nhịp ngay lập tức: phân giải DNS và xác thực có thể mất vài giây trước dòng đầu
    // tiên của git, và một hộp thoại không nhúc nhích trông y hệt một hộp thoại treo.
    if tx
        .send(CloneEvent::Phase {
            label: "Đang tạo bản sao".to_string(),
        })
        .await
        .is_err()
    {
        giet_nhom(&mut con).await;
        return;
    }

    let mut cuoi = VecDeque::new();
    let ket = tokio::select! {
        // `closed()` là chỗ phát hiện luồng bị thả trong lúc git đang im lặng chờ mạng.
        // Chỉ trông vào `send` báo lỗi thì một bản clone treo sẽ không bao giờ bị huỷ.
        _ = tx.closed() => Ket::Huy,
        ket = bom(&mut con, stderr, &tx, &mut cuoi) => ket,
    };

    match ket {
        Ket::Huy => {
            giet_nhom(&mut con).await;
            // Dọn phần đã tải dở: để lại một thư mục nửa vời thì lần thử lại tiếp theo
            // đâm vào chính câu "thư mục đích không rỗng" ở trên và người dùng bế tắc.
            let _ = std::fs::remove_dir_all(&dich);
            if da_co_san {
                let _ = std::fs::create_dir_all(&dich);
            }
        }
        Ket::Loi(message) => {
            giet_nhom(&mut con).await;
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
            // Kèm mấy dòng cuối của git: "mã 128" một mình không nói được người dùng gõ
            // sai URL hay chưa có quyền đọc repo.
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

/// Đọc stderr theo từng mảnh và dịch thành sự kiện.
///
/// Đọc byte chứ không đọc dòng: `AsyncBufReadExt::lines` tách theo `\n`, mà git ghi đè
/// một dòng tiến trình bằng `\r`. Gộp mảnh vào một bộ đệm byte rồi mới cắt cũng là cố ý —
/// một ký tự tiếng Việt có thể nằm vắt qua hai lần đọc, và cắt giữa nó là một dấu hỏi
/// trong khung chi tiết.
async fn bom(
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
            ghi_nho(cuoi, &text);
            for su_kien in dich_dong(&text, &mut pha) {
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

/// Chỉ giữ mấy dòng gần nhất: thông báo lỗi cần ngữ cảnh, không cần cả bản ghi.
fn ghi_nho(cuoi: &mut VecDeque<String>, text: &str) {
    cuoi.push_back(text.to_string());
    if cuoi.len() > 5 {
        cuoi.pop_front();
    }
}

fn dich_dong(text: &str, pha: &mut String) -> Vec<CloneEvent> {
    // `remote: ` đứng trước những dòng do máy chủ đếm; bỏ đi để phần còn lại phân tích
    // được như dòng của máy mình.
    let sach = text.strip_prefix("remote: ").unwrap_or(text).trim();
    match doc_tien_do(sach) {
        Some((goc, percent)) => {
            let label = nhan_pha(&goc);
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
fn doc_tien_do(dong: &str) -> Option<(String, u8)> {
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

/// Tên pha của git, bằng tiếng Việt.
///
/// Không dịch được thì trả nguyên văn: một bản git khác đặt tên pha khác, và nuốt mất
/// dòng đó nghĩa là thanh tiến trình đứng im ở một pha có thật.
fn nhan_pha(goc: &str) -> String {
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

/// SIGTERM cho cả nhóm, chờ một nhịp, rồi SIGKILL.
///
/// Giết mỗi `git` thì `git-remote-https` và `ssh` con của nó sống tiếp, vẫn tải, vẫn ghi
/// vào thư mục ta sắp xoá. Số pid âm nghĩa là "cả nhóm" — đó là toàn bộ lý do đặt
/// `process_group(0)` lúc spawn.
async fn giet_nhom(con: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = con.id() {
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
    let _ = con.kill().await;
}
