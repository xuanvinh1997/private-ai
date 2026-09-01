//! Chặn URL độc, và một bản clone thật.

use std::path::{Path, PathBuf};
use std::process::Command;

use futures::StreamExt;
use pai_project::{CloneEvent, CloneRequest, clone};
use tempfile::TempDir;

fn yeu_cau(url: &str, parent: &Path) -> CloneRequest {
    CloneRequest {
        url: url.to_string(),
        parent: parent.to_path_buf(),
        name: None,
        depth: None,
    }
}

/// `git clone "ext::sh -c '...'"` không tải gì cả — nó chạy lệnh đó trên máy người dùng.
///
/// Đây là lý do tồn tại của `validate()`. Một URL dán vào ô "clone" là một dòng lệnh nếu
/// không có câu chặn này.
#[test]
fn transport_helper_bi_chan_vi_no_la_thi_hanh_lenh() {
    let dir = TempDir::new().expect("thư mục tạm");
    let loi = yeu_cau("ext::sh -c id", dir.path())
        .validate()
        .expect_err("phải bị chặn");
    assert!(loi.to_string().contains("ext"), "lỗi phải nói rõ: {loi}");

    // Không chỉ `ext::` — mọi helper, vì danh sách helper mở rộng được.
    assert!(yeu_cau("khac::gi-do", dir.path()).validate().is_err());
    // Nhưng `::` trong đường dẫn của một URL bình thường thì không phải helper.
    assert!(
        yeu_cau("https://vi.du/a::b.git", dir.path())
            .validate()
            .is_ok(),
        "chặn nhầm một URL hợp lệ"
    );
}

#[test]
fn url_bat_dau_bang_gach_ngang_bi_chan() {
    let dir = TempDir::new().expect("thư mục tạm");
    let loi = yeu_cau("--upload-pack=id", dir.path())
        .validate()
        .expect_err("phải bị chặn");
    assert!(loi.to_string().contains('-'), "lỗi phải nói rõ: {loi}");
}

#[test]
fn scheme_la_bi_chan() {
    let dir = TempDir::new().expect("thư mục tạm");
    for url in ["ftp://vi.du/x.git", "javascript://x", "/nha/repo", ""] {
        assert!(
            yeu_cau(url, dir.path()).validate().is_err(),
            "`{url}` lọt qua"
        );
    }
    for url in [
        "https://vi.du/x.git",
        "http://vi.du/x.git",
        "ssh://git@vi.du/x.git",
        "git://vi.du/x.git",
        "file:///nha/repo",
        "git@vi.du:nhom/x.git",
    ] {
        assert!(
            yeu_cau(url, dir.path()).validate().is_ok(),
            "`{url}` bị chặn oan"
        );
    }
}

#[test]
fn ten_thoat_khoi_thu_muc_cha_bi_chan() {
    let dir = TempDir::new().expect("thư mục tạm");
    for ten in ["..", "../ngoai", "a/b", "a\\b", ""] {
        let mut req = yeu_cau("https://vi.du/x.git", dir.path());
        req.name = Some(ten.to_string());
        assert!(req.validate().is_err(), "tên `{ten}` lọt qua");
        assert!(req.destination().is_err(), "tên `{ten}` vẫn dựng ra đích");
    }

    let mut req = yeu_cau("https://vi.du/x.git", dir.path());
    req.name = Some("noi-khac".to_string());
    assert_eq!(
        req.destination().expect("tên hợp lệ"),
        dir.path().join("noi-khac")
    );
    // Không đặt tên thì suy từ URL, và `.git` ở cuối bị bỏ.
    assert_eq!(
        yeu_cau("https://vi.du/nhom/x.git", dir.path())
            .destination()
            .expect("suy được tên"),
        dir.path().join("x")
    );
}

#[test]
fn thu_muc_dich_co_du_lieu_thi_khong_clone_de_len() {
    let dir = TempDir::new().expect("thư mục tạm");
    let dich = dir.path().join("x");
    std::fs::create_dir(&dich).expect("tạo");
    std::fs::write(dich.join("cua-toi.txt"), "còn").expect("ghi");

    let loi = yeu_cau("https://vi.du/x.git", dir.path())
        .validate()
        .expect_err("phải bị chặn");
    assert!(
        loi.to_string().contains("mất dữ liệu"),
        "lỗi mờ nhạt: {loi}"
    );

    // Thư mục rỗng thì được: đó là chỗ người dùng vừa tạo để clone vào.
    std::fs::remove_file(dich.join("cua-toi.txt")).expect("xoá");
    assert!(
        yeu_cau("https://vi.du/x.git", dir.path())
            .validate()
            .is_ok()
    );
}

fn git(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .map(|trang_thai| trang_thai.success())
        .unwrap_or(false)
}

fn co_git() -> bool {
    Command::new("git")
        .arg("--version")
        .status()
        .map(|trang_thai| trang_thai.success())
        .unwrap_or(false)
}

/// Dựng một repo nguồn có đúng một commit, trả về URL `file://` của nó.
fn repo_nguon(goc: &Path) -> Option<String> {
    let nguon = goc.join("nguon");
    std::fs::create_dir(&nguon).ok()?;
    if !git(&nguon, &["init", "-q"]) {
        return None;
    }
    std::fs::write(nguon.join("xin-chao.txt"), "xin chào").ok()?;
    if !git(&nguon, &["add", "."]) {
        return None;
    }
    // Máy chạy CI có thể chưa cấu hình danh tính; đặt tại chỗ để commit không hỏi gì.
    let commit = git(
        &nguon,
        &[
            "-c",
            "user.email=test@vi.du",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "dau tien",
        ],
    );
    if !commit {
        return None;
    }
    let that = nguon.canonicalize().ok()?;
    Some(format!("file://{}", that.display()))
}

#[tokio::test]
async fn clone_that_phat_tien_do_roi_ket_thuc_bang_done() {
    if !co_git() {
        eprintln!("bỏ qua: máy này không có `git` trong PATH");
        return;
    }
    let dir = TempDir::new().expect("thư mục tạm");
    let Some(url) = repo_nguon(dir.path()) else {
        eprintln!("bỏ qua: không dựng được repo nguồn bằng `git`");
        return;
    };
    let cha = dir.path().join("dich");
    std::fs::create_dir(&cha).expect("tạo thư mục chứa");

    let mut luong = clone(CloneRequest {
        url,
        parent: cha.clone(),
        name: Some("ban-sao".to_string()),
        depth: None,
    });

    let mut co_nhip = false;
    let mut xong: Option<PathBuf> = None;
    while let Some(su_kien) = luong.next().await {
        match su_kien {
            CloneEvent::Phase { .. } | CloneEvent::Progress { .. } => co_nhip = true,
            CloneEvent::Line { .. } => {}
            CloneEvent::Done { path } => xong = Some(path),
            CloneEvent::Failed { message } => panic!("clone hỏng: {message}"),
        }
    }

    assert!(co_nhip, "luồng không phát nhịp nào — giao diện sẽ đứng im");
    let path = xong.expect("phải kết thúc bằng Done");
    assert_eq!(path, cha.join("ban-sao"));
    assert!(
        path.join("xin-chao.txt").exists(),
        "clone xong mà tệp không có mặt"
    );
}

/// URL bị chặn phải kết thúc luồng bằng `Failed`, không phải bằng im lặng.
///
/// Một luồng không bao giờ phát gì trông y hệt một bản clone đang chạy chậm, và giao diện
/// sẽ quay vòng mãi mãi.
#[tokio::test]
async fn url_hong_thi_luong_ket_thuc_bang_failed_chu_khong_treo() {
    let dir = TempDir::new().expect("thư mục tạm");
    let mut luong = clone(yeu_cau("ext::sh -c id", dir.path()));
    let dau_tien = luong.next().await.expect("phải có sự kiện");
    assert!(
        matches!(dau_tien, CloneEvent::Failed { .. }),
        "{dau_tien:?}"
    );
    assert!(luong.next().await.is_none(), "hỏng rồi thì phải đóng luồng");
}
