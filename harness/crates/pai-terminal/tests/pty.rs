//! Invariants of a persistent terminal, six tests each locking one silent-failure mode:
//! sessions persist, the PTY is real, closing kills the whole tree, the buffer caps and reports
//! drops, sessions belong to their owner, and unloading the plugin closes everything.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pai_core::{Context, Plugin};
use pai_sandbox::Policy;
use pai_terminal::provider::LocalTerminals;
use pai_terminal::seam::{OpenRequest, Owner, Sent, TerminalHost, Terminals, Wait};
use pai_terminal::{TerminalPlugin, seam::Signal};
use pai_tools::{Tools, ToolsPlugin};

/// Terminals without a sandbox: these tests check the PTY and the process tree, and a wrapper would measure something else.
fn terminals(max_lines: usize) -> Arc<LocalTerminals> {
    Arc::new(
        LocalTerminals::new(
            Context::root(),
            Policy::danger_full_access("/tmp"),
            PathBuf::from("/tmp"),
        )
        .with_max_lines(max_lines),
    )
}

fn wait() -> Option<Wait> {
    Some(Wait {
        quiet: Duration::from_millis(300),
        timeout: Duration::from_secs(20),
    })
}

async fn open(host: &Arc<LocalTerminals>, owner: Owner) -> String {
    host.open(
        owner,
        OpenRequest {
            backend: "shell".into(),
            name: None,
            cwd: None,
        },
    )
    .await
    .expect("mở được phiên")
    .id
}

async fn run(host: &Arc<LocalTerminals>, owner: Owner, id: &str, line: &str) -> Sent {
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\n');
    host.send(owner, id, &bytes, wait())
        .await
        .expect("gửi được")
}

fn joined(sent: &Sent) -> String {
    sent.lines.join("\n")
}

#[tokio::test]
async fn cd_o_lan_goi_truoc_con_tac_dung_o_lan_sau() {
    let host = terminals(1_000);
    let id = open(&host, None).await;

    // A real directory that is not the initial cwd, so `pwd` cannot be accidentally right.
    run(&host, None, &id, "mkdir -p /tmp/pai-term-cd/sau").await;
    run(&host, None, &id, "cd /tmp/pai-term-cd/sau").await;

    // A different call: if each call were a fresh process this would print `/tmp`.
    let sau = run(&host, None, &id, "pwd").await;
    assert!(
        joined(&sau).contains("/tmp/pai-term-cd/sau"),
        "cd không sống qua lần gọi:\n{}",
        joined(&sau)
    );

    host.close(None, &id).await.expect("đóng được");
}

#[tokio::test]
async fn chuong_trinh_hoi_isatty_thay_co_terminal() {
    let host = terminals(1_000);
    let id = open(&host, None).await;

    let sent = run(
        &host,
        None,
        &id,
        "test -t 1 && echo co-tty || echo khong-tty",
    )
    .await;
    // Match whole lines, not substrings: the PTY echoes the command back, and that echo contains both keywords.
    let text = joined(&sent);
    let says = |what: &str| sent.lines.iter().any(|line| line.trim() == what);
    assert!(
        says("co-tty"),
        "đầu ra không phải terminal — công cụ sẽ tắt màu và đổi cách hỏi:\n{text}"
    );
    assert!(!says("khong-tty"), "{text}");

    host.close(None, &id).await.expect("đóng được");
}

#[tokio::test]
async fn dong_phien_giet_ca_chau_cua_no() {
    let marker = std::env::temp_dir().join(format!("pai-term-chau-{}", stamp()));
    let _ = std::fs::remove_file(&marker);

    let host = terminals(1_000);
    let id = open(&host, None).await;

    // The grandchild waits 30s before touching the marker: surviving the close creates the file, dying with the session never does.
    run(
        &host,
        None,
        &id,
        &format!("(sleep 30; touch {}) &", marker.display()),
    )
    .await;

    host.close(None, &id).await.expect("đóng được");

    // Wait past the point where the grandchild would touch the file. It must not.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !marker.exists(),
        "cháu sống sót qua lần đóng: {}",
        marker.display()
    );
}

#[tokio::test]
async fn bo_dem_vuot_tran_giu_phan_moi_va_noi_ra_phan_da_bo() {
    let host = terminals(20);
    let id = open(&host, None).await;

    run(
        &host,
        None,
        &id,
        "for i in $(seq 1 300); do echo dong-$i; done",
    )
    .await;

    let page = host.read(None, &id, 0, 500).expect("đọc được");
    assert!(page.dropped > 0, "trần không có tác dụng nào");
    // The cap plus exactly one pending line: the shell prompt never ends in `\n` yet signals readiness.
    assert!(page.lines.len() <= 21, "giữ quá trần: {}", page.lines.len());

    let text = page.lines.join("\n");
    assert!(text.contains("dong-300"), "mất phần mới nhất:\n{text}");
    assert!(!text.contains("dong-1\n"), "giữ phần cũ nhất:\n{text}");
    assert!(!text.contains("dong-100\n"), "giữ phần cũ:\n{text}");

    host.close(None, &id).await.expect("đóng được");
}

#[tokio::test]
async fn phien_cua_pham_vi_nay_khong_nhin_thay_duoc_tu_pham_vi_khac() {
    let root = Context::root();
    let mot: Owner = root.scoped("agent-mot").scope_key();
    let hai: Owner = root.scoped("agent-hai").scope_key();
    assert_ne!(mot, hai);

    let host = terminals(1_000);
    let id = open(&host, mot).await;

    // Another owner holding the right id still cannot read, write or close.
    assert!(host.read(hai, &id, 0, 10).is_err());
    assert!(host.send(hai, &id, b"echo x\n", None).await.is_err());
    assert!(host.signal(hai, &id, Signal::Int).is_err());
    assert!(host.close(hai, &id).await.is_err());
    assert!(
        host.list(hai).is_empty(),
        "phiên của chủ khác lọt vào danh sách"
    );

    // And the session is intact for its real owner: the refusals above must not have closed it.
    assert_eq!(host.list(mot).len(), 1);
    assert!(host.read(mot, &id, 0, 10).is_ok());

    host.close(mot, &id).await.expect("đóng được");
}

#[tokio::test]
async fn go_plugin_dong_sach_moi_phien() {
    let marker = std::env::temp_dir().join(format!("pai-term-go-{}", stamp()));
    let _ = std::fs::remove_file(&marker);

    let root = Context::root();
    let tools_ctx = root.plugin("tools");
    ToolsPlugin.apply(&tools_ctx).await.expect("cắm được tools");

    let terminal_ctx = root.plugin("terminal");
    TerminalPlugin::new(PathBuf::from("/tmp"))
        .apply(&terminal_ctx)
        .await
        .expect("cắm được terminal");

    let host = root.require::<Terminals>().expect("seam có mặt");
    let id = open_via(&host, None).await;
    host.send(
        None,
        &id,
        format!("(sleep 30; touch {}) &\n", marker.display()).as_bytes(),
        wait(),
    )
    .await
    .expect("gửi được");

    // Six tools, no more and no fewer: a tool vanishing silently is a capability vanishing silently.
    let names: Vec<String> = root
        .require::<Tools>()
        .expect("sổ tool có mặt")
        .schemas(None)
        .into_iter()
        .map(|schema| schema.name.as_str().to_string())
        .filter(|name| name.starts_with("terminal_"))
        .collect();
    assert_eq!(names.len(), 6, "{names:?}");

    terminal_ctx.effects().dispose().await;

    // The seam goes with the plugin, and the process tree goes with the seam.
    assert!(
        root.get::<Terminals>().is_none(),
        "seam sống sót qua lần gỡ"
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(
        !marker.exists(),
        "phiên sống sót qua lần gỡ plugin: {}",
        marker.display()
    );
}

async fn open_via(host: &Arc<dyn TerminalHost>, owner: Owner) -> String {
    host.open(
        owner,
        OpenRequest {
            backend: "shell".into(),
            name: None,
            cwd: None,
        },
    )
    .await
    .expect("mở được phiên")
    .id
}

/// Unique enough for a temp file name; nothing more is needed.
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}
