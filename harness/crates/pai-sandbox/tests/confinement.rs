//! Vòng giam có thật không.
//!
//! Bài ở đây cố ý **chạy lệnh thật** thay vì so chuỗi hồ sơ. Một hồ sơ SBPL đúng cú pháp
//! nhưng sai ngữ nghĩa vẫn khớp mọi phép so chuỗi và vẫn không giam gì cả — SBPL lấy luật
//! khớp *cuối cùng*, nên chỉ cần đảo thứ tự hai mệnh đề là hồ sơ trở thành vô hại mà nhìn
//! vẫn y hệt.

#![cfg(target_os = "macos")]

use std::path::Path;
use std::process::{Command, Stdio};

use pai_sandbox::seam::SandboxProvider;
use pai_sandbox::{Mode, Policy};
use tempfile::TempDir;

/// Chạy một lệnh shell qua vòng giam. Trả về `true` nếu nó thành công.
fn runs(policy: &Policy, command: &str) -> bool {
    let Some(seatbelt) = pai_sandbox::seatbelt::Seatbelt::detect() else {
        // Không dò được thì bỏ qua chứ không báo đỏ: máy CI trong App Sandbox không chạy
        // được `sandbox-exec`, và một bài đỏ vì môi trường là một bài không ai còn tin.
        eprintln!("bỏ qua: máy này không chạy được sandbox-exec");
        return true;
    };
    let argv = seatbelt
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

fn bench() -> (TempDir, TempDir) {
    (
        TempDir::new().expect("workspace"),
        TempDir::new().expect("ngoài workspace"),
    )
}

#[test]
fn workspace_write_cho_ghi_trong_workspace() {
    let (workspace, _) = bench();
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
    let (workspace, _) = bench();
    let root = workspace.path().canonicalize().expect("phân giải");
    let policy = Policy::workspace_write(&root);

    // **Không** dùng `TempDir` làm chỗ "ngoài workspace": `writable_roots` cố ý cho ghi
    // cả thư mục tạm, nên một tệp tạm nằm ngay trong vùng được phép và bài sẽ đo nhầm
    // rằng sandbox không giam. Nhà của người dùng thì thật sự ở ngoài.
    let home = std::env::var("HOME").expect("có HOME");
    let target = Path::new(&home).join(format!(
        ".pai-sandbox-khong-duoc-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&target);

    assert!(
        !runs(&policy, &format!("echo x > {}", target.display())),
        "ghi ra ngoài workspace phải thất bại"
    );
    // Và nó thất bại vì bị chặn, không phải vì lệnh sai: tệp không được sinh ra.
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
    let (workspace, _) = bench();
    let root = workspace.path().canonicalize().expect("phân giải");
    let policy = Policy::read_only(&root);

    assert!(!runs(
        &policy,
        &format!("echo x > {}/a.txt", root.display())
    ));
    // Nhưng đọc vẫn phải chạy: một agent chỉ-đọc không đọc được thì vô dụng.
    assert!(runs(&policy, "/bin/ls / > /dev/null"));
}

#[test]
fn danger_full_access_khong_boc_gi_ca() {
    let (workspace, _) = bench();
    let policy = Policy::danger_full_access(workspace.path());
    let argv = vec!["/bin/echo".to_string(), "chao".to_string()];

    let seatbelt = pai_sandbox::seatbelt::Seatbelt::with_runner("/usr/bin/sandbox-exec");
    let wrapped = seatbelt.wrap(argv.clone(), &policy).expect("bọc được");
    // Chế độ này là *sự vắng mặt* của sandbox. Bọc nó là dựng một vòng vây rỗng rồi phải
    // nuôi nó qua từng bản phát hành.
    assert_eq!(wrapped, argv);
    assert_eq!(policy.mode, Mode::DangerFullAccess);
}

#[test]
fn ho_so_khong_giam_thi_khong_duoc_bao_cao_la_giam() {
    // `Enforcement` là sự thật báo cáo, không phải lời hứa: một sandbox nói dối nguy hiểm
    // hơn hẳn không có sandbox, vì người dùng bấm "cho phép" dựa trên nó.
    let unconfined = pai_sandbox::Unconfined::new("máy này không có gì để giam");
    assert!(!unconfined.enforcement().confines());
    assert!(unconfined.enforcement().reason().is_some());
}
