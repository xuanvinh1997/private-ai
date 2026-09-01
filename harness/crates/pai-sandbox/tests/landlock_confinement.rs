//! Vòng giam Linux có thật không.
//!
//! Giống bài macOS, những bài này **chạy lệnh thật** thay vì so chuỗi tham số: một chính
//! sách dựng đúng cú pháp nhưng sai ngữ nghĩa vẫn khớp mọi phép so chuỗi và vẫn không
//! giam gì cả.
//!
//! Chạy chúng cần một kernel có Landlock **và** không bị seccomp chặn syscall. Trong
//! Docker mặc định thì bị chặn — và đó không phải lý do để bỏ qua, mà là một trường hợp
//! kiểm chứng: lúc đó provider phải báo `None` kèm lý do chứ không im lặng cho qua.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Command, Stdio};

use pai_sandbox::landlock::Landlock;
use pai_sandbox::seam::{Enforcement, SandboxProvider};
use pai_sandbox::{Mode, Policy};
use tempfile::TempDir;

/// Binary trung gian do chính bộ test build.
const RUNNER: &str = env!("CARGO_BIN_EXE_pai-landlock-run");

fn provider() -> Landlock {
    Landlock::with_runner(RUNNER)
}

/// Kernel này giam được không. Không giam được thì bài bỏ qua thay vì báo đỏ — một bộ
/// test đỏ vì môi trường là một bộ test không ai còn tin.
fn giam_duoc() -> bool {
    match provider().enforcement() {
        Enforcement::None(reason) => {
            eprintln!("bỏ qua: {reason}");
            false
        }
        _ => true,
    }
}

fn runs(policy: &Policy, command: &str) -> bool {
    let argv = provider()
        .wrap(vec!["/bin/sh".into(), "-c".into(), command.into()], policy)
        .expect("bọc được argv");
    let (program, args) = argv.split_first().expect("argv không rỗng");
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn workspace_write_cho_ghi_trong_workspace() {
    if !giam_duoc() {
        return;
    }
    let workspace = TempDir::new().expect("thư mục tạm");
    let root = workspace.path().canonicalize().expect("phân giải");
    let policy = Policy::workspace_write(&root);
    assert!(
        runs(
            &policy,
            &format!("echo xin-chao > {}/a.txt", root.display())
        ),
        "ghi trong workspace phải chạy được, nếu không thì agent không sửa được repo"
    );
}

#[test]
fn workspace_write_chan_ghi_ngoai_workspace() {
    if !giam_duoc() {
        return;
    }
    let workspace = TempDir::new().expect("thư mục tạm");
    let root = workspace.path().canonicalize().expect("phân giải");
    let policy = Policy::workspace_write(&root);

    // **Không** dùng `TempDir` hay `/var/tmp` làm chỗ "ngoài workspace": `writable_roots`
    // cố ý cho ghi cả `/tmp` lẫn `/var/tmp`, nên một tệp ở đó nằm ngay trong vùng được
    // phép và bài sẽ đo nhầm rằng sandbox không giam. Nhà của người dùng thì thật sự ở
    // ngoài.
    let home = std::env::var("HOME").expect("có HOME");
    let target = Path::new(&home).join(format!("pai-khong-duoc-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&target);

    assert!(
        !runs(&policy, &format!("echo x > {}", target.display())),
        "ghi ra ngoài workspace phải thất bại"
    );
    let leaked = target.exists();
    let _ = std::fs::remove_file(&target);
    assert!(
        !leaked,
        "tệp bị chặn nhưng vẫn xuất hiện: {}",
        target.display()
    );
}

#[test]
fn read_only_chan_ghi_ngay_ca_trong_workspace() {
    if !giam_duoc() {
        return;
    }
    let workspace = TempDir::new().expect("thư mục tạm");
    let root = workspace.path().canonicalize().expect("phân giải");
    let policy = Policy::read_only(&root);

    assert!(!runs(
        &policy,
        &format!("echo x > {}/a.txt", root.display())
    ));
    // Nhưng đọc vẫn phải chạy: một agent chỉ-đọc không đọc được thì vô dụng. Lệnh này
    // cũng là bài kiểm cho cống `/dev/null` — gần như mọi lệnh đều mở nó để vứt output.
    assert!(runs(&policy, "/bin/ls / > /dev/null"));
}

#[test]
fn danger_full_access_khong_boc_gi_ca() {
    let workspace = TempDir::new().expect("thư mục tạm");
    let policy = Policy::danger_full_access(workspace.path());
    let argv = vec!["/bin/echo".to_string(), "chao".to_string()];

    // Chế độ này là *sự vắng mặt* của sandbox; bọc nó là dựng một vòng vây rỗng rồi phải
    // nuôi. Bài này chạy được cả khi kernel không có Landlock — nó không đụng tới kernel.
    assert_eq!(
        provider().wrap(argv.clone(), &policy).expect("bọc được"),
        argv
    );
    assert_eq!(policy.mode, Mode::DangerFullAccess);
}

#[test]
fn khong_giam_duoc_thi_tu_choi_chay_chu_khong_chay_tran() {
    let workspace = TempDir::new().expect("thư mục tạm");
    let policy = Policy::workspace_write(workspace.path());
    // Runner không tồn tại ⇒ `enforcement()` là `None` ⇒ `wrap` phải **từ chối**.
    let broken = Landlock::with_runner("/khong-co-tep-nay");
    let err = broken
        .wrap(vec!["/bin/echo".into(), "chao".into()], &policy)
        .expect_err("không giam được thì không chạy");
    // Trả lại argv trần ở đây là lặng lẽ bỏ vòng vây đúng lúc người dùng tin là có.
    assert!(err.to_string().contains("không giam được"), "{err}");
}
