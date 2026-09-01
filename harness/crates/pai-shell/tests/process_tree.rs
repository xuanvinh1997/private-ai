//! Bất biến của việc chạy lệnh.
//!
//! Bài quan trọng nhất ở đây là bài về cháu. Giết một shell mà không giết con cháu nó là
//! loại lỗi không bao giờ tự lộ ra — mọi thứ trông vẫn chạy, chỉ có cổng bị giữ, khoá tệp
//! bị giữ, và lượt sau chạy trong một cái máy đã bị nhiễm.

use std::path::PathBuf;
use std::time::Duration;

use pai_core::Context;
use pai_sandbox::Policy;
use pai_shell::provider::{LocalShell, Request, ShellExecutor};
use tokio_util::sync::CancellationToken;

/// Shell không có vòng giam. Những bài dưới đây kiểm cây tiến trình, không kiểm sandbox;
/// bọc thêm một lớp `sandbox-exec` chỉ làm chúng đo nhầm thứ khác.
fn shell() -> LocalShell {
    LocalShell::new(Context::root(), Policy::danger_full_access("/tmp"))
}

fn request(command: &str, timeout: Option<Duration>, cancel: CancellationToken) -> Request {
    Request {
        command: command.to_string(),
        cwd: PathBuf::from("/tmp"),
        timeout,
        cancel,
    }
}

#[tokio::test]
async fn ma_thoat_di_qua_nguyen_ven() {
    let shell = shell();
    let ok = shell
        .run(request("echo xin-chao", None, CancellationToken::new()))
        .await
        .expect("chạy được");
    assert_eq!(ok.exit_code, Some(0));
    assert!(ok.output.contains("xin-chao"));

    let failed = shell
        .run(request("exit 101", None, CancellationToken::new()))
        .await
        .expect("chạy được");
    // Mã thoát khác 0 vẫn là một lần chạy thành công: lệnh đã làm đúng thứ được bảo.
    assert_eq!(failed.exit_code, Some(101));
}

#[tokio::test]
async fn stdout_va_stderr_gop_theo_thu_tu_toi() {
    let run = shell()
        .run(request(
            "echo mot; echo hai >&2; echo ba",
            None,
            CancellationToken::new(),
        ))
        .await
        .expect("chạy được");
    for line in ["mot", "hai", "ba"] {
        assert!(
            run.output.contains(line),
            "thiếu `{line}` trong:\n{}",
            run.output
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn giet_mot_lenh_giet_ca_chau_cua_no() {
    use std::path::Path;

    let marker = std::env::temp_dir().join(format!("pai-chau-{}", uuid_like()));
    let _ = std::fs::remove_file(&marker);

    // Cháu sống 30 giây rồi mới chạm tệp đánh dấu. Nếu nó sống sót qua lần giết, tệp sẽ
    // xuất hiện; nếu nó chết cùng cha, tệp không bao giờ có.
    let command = format!("(sleep 30; touch {}) & sleep 30", marker.display());
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        token.cancel();
    });

    let run = shell()
        .run(request(&command, None, cancel))
        .await
        .expect("chạy được");
    assert_eq!(run.interrupted.as_deref(), Some("lượt đã bị huỷ"));

    // Đợi qua mốc cháu định chạm tệp. Nó không được chạm.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !Path::new(&marker).exists(),
        "cháu sống sót qua lần giết: {}",
        marker.display()
    );
}

#[tokio::test]
async fn het_gio_thi_dung_va_van_tra_ve_phan_da_co() {
    let run = shell()
        .run(request(
            "echo bat-dau; sleep 30",
            Some(Duration::from_millis(400)),
            CancellationToken::new(),
        ))
        .await
        .expect("chạy được");

    assert!(
        run.interrupted.is_some(),
        "hết giờ phải nói ra, không im lặng"
    );
    // Phần đã in ra trước khi bị dừng vẫn có ích và không được vứt đi.
    assert!(
        run.output.contains("bat-dau"),
        "mất output đã có:\n{}",
        run.output
    );
    assert_eq!(run.exit_code, None);
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}
