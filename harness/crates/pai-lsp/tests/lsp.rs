//! The invariants whose failure would make the model believe something untrue about the code.
//! No test needs a real language server: the server here is an in-process task speaking
//! the protocol over a [`tokio::io::duplex`], so the suite runs on any CI machine.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use pai_core::{Context, Plugin};
use pai_fs::FileRoots;
use pai_lsp::{
    Channel, Entry, LanguageConfig, Launch, Limits, LspPlugin, LspTool, StdioServers, from_uri,
    proto, to_uri,
};
use pai_tools::{Invocation, Tool, ToolOutcome, Tools, ToolsPlugin};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use tempfile::TempDir;

const NGUON: &str =
    "fn xin_chao() {\n    println!(\"chào\");\n}\n\nfn main() {\n    xin_chao();\n}\n";

// --- fake server ----------------------------------------------------------------------

/// The fake server's script. Each flag is one failure mode a test aims at.
#[derive(Clone)]
struct Plan {
    /// Methods received, in wire order. This is the evidence for the handshake invariant.
    seen: Arc<Mutex<Vec<String>>>,
    /// Never answer `initialize` - a server loading a huge workspace.
    answer_initialize: bool,
    /// Close the pipe as soon as a query arrives - a server dying mid-flight.
    die_on_query: bool,
    /// Echo the URI back. Proves a path survives the full round trip intact.
    echo_definition: bool,
    /// Publish diagnostics after the file is opened.
    publish: Option<Value>,
    /// Stay busy: send `$/progress` `begin` and never `end`.
    stay_busy: bool,
}

impl Plan {
    fn new() -> Plan {
        Plan {
            seen: Arc::new(Mutex::new(Vec::new())),
            answer_initialize: true,
            die_on_query: false,
            echo_definition: true,
            publish: None,
            stay_busy: false,
        }
    }
}

async fn serve(stream: tokio::io::DuplexStream, plan: Plan) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(reader);

    loop {
        let message = match proto::read_message(&mut reader).await {
            Ok(Some(message)) => message,
            _ => return,
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id = message.get("id").and_then(Value::as_i64);
        plan.seen.lock().push(method.clone());

        let reply = |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });

        match method.as_str() {
            "initialize" => {
                if !plan.answer_initialize {
                    continue;
                }
                let _ =
                    proto::write_message(&mut writer, &reply(json!({ "capabilities": {} }))).await;
                if plan.stay_busy {
                    let _ = proto::write_message(
                        &mut writer,
                        &json!({
                            "jsonrpc": "2.0", "method": "$/progress",
                            "params": { "token": "nap-du-an", "value": { "kind": "begin", "title": "indexing" } },
                        }),
                    )
                    .await;
                }
            }
            "textDocument/didOpen" => {
                if let Some(diagnostics) = &plan.publish {
                    let uri = message
                        .pointer("/params/textDocument/uri")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let _ = proto::write_message(
                        &mut writer,
                        &json!({
                            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                            "params": { "uri": uri, "diagnostics": diagnostics },
                        }),
                    )
                    .await;
                }
            }
            "textDocument/definition" | "textDocument/references" => {
                if plan.die_on_query {
                    // Drop both ends of the pipe: exactly like a process the OS just killed.
                    return;
                }
                let uri = message
                    .pointer("/params/textDocument/uri")
                    .cloned()
                    .unwrap_or(Value::Null);
                let result = if plan.echo_definition {
                    json!([{
                        "uri": uri,
                        "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 11 } },
                    }])
                } else {
                    Value::Null
                };
                let _ = proto::write_message(&mut writer, &reply(result)).await;
            }
            "textDocument/hover" => {
                let _ = proto::write_message(
                    &mut writer,
                    &reply(json!({ "contents": { "kind": "markdown", "value": "fn xin_chao()" } })),
                )
                .await;
            }
            "shutdown" => {
                let _ = proto::write_message(&mut writer, &reply(Value::Null)).await;
            }
            "exit" => return,
            _ => {
                if let Some(id) = id {
                    let _ = proto::write_message(
                        &mut writer,
                        &json!({
                            "jsonrpc": "2.0", "id": id,
                            "error": { "code": -32601, "message": "server giả không cài" },
                        }),
                    )
                    .await;
                }
            }
        }
    }
}

/// Opens a pipe to an in-memory fake server. No child process, no `PATH`, nothing installed.
struct FakeLaunch {
    plan: Plan,
    launched: Arc<AtomicBool>,
}

#[async_trait]
impl Launch for FakeLaunch {
    async fn launch(&self) -> anyhow::Result<Channel> {
        self.launched.store(true, Ordering::SeqCst);
        let (ours, theirs) = tokio::io::duplex(16 * 1024);
        tokio::spawn(serve(theirs, self.plan.clone()));
        let (reader, writer) = tokio::io::split(ours);
        Ok(Channel::new(reader, writer))
    }

    fn label(&self) -> String {
        "server-gia".into()
    }
}

// --- harness --------------------------------------------------------------------------

struct Rig {
    _dir: TempDir,
    root: PathBuf,
    tool: LspTool,
    plan: Plan,
}

fn rig_with(plan: Plan, limits: Limits, file_name: &str) -> Rig {
    let dir = TempDir::new().expect("dựng được thư mục tạm");
    let root = dir.path().canonicalize().expect("chuẩn hoá được gốc");
    std::fs::write(root.join(file_name), NGUON).expect("ghi được tệp nguồn");

    let servers = Arc::new(StdioServers::new(
        root.clone(),
        FileRoots::new([root.clone()], []),
        vec![Entry {
            id: "rust".into(),
            extensions: vec!["rs".into()],
            launcher: Arc::new(FakeLaunch {
                plan: plan.clone(),
                launched: Arc::new(AtomicBool::new(false)),
            }),
            options: None,
        }],
        limits,
    ));

    Rig {
        _dir: dir,
        root,
        tool: LspTool::new(servers),
        plan,
    }
}

fn nhanh() -> Limits {
    Limits {
        startup: Duration::from_millis(400),
        request: Duration::from_millis(800),
        diagnostics: Duration::from_millis(400),
        max_locations: 100,
    }
}

async fn goi(tool: &LspTool, args: Value) -> ToolOutcome {
    let arguments: Map<String, Value> = args.as_object().cloned().expect("tham số là object");
    let call = Invocation::new("lsp".into(), "goi-1", arguments);
    // A hard ceiling around every call: a hanging test says nothing, and "does not hang" is what we are testing.
    tokio::time::timeout(Duration::from_secs(10), tool.execute(&call))
        .await
        .expect("tool trả lời trong mười giây")
        .unwrap_or_else(|err| ToolOutcome::error(err.to_string()))
}

// --- invariants -----------------------------------------------------------------------

/// No query goes before the handshake; wire order is the only acceptable evidence, since a correct in-memory flag with wrong wire order still breaks the server.
#[tokio::test]
async fn khong_hoi_gi_truoc_khi_bat_tay_xong() {
    let rig = rig_with(Plan::new(), nhanh(), "nguon.rs");
    let outcome = goi(
        &rig.tool,
        json!({ "operation": "definition", "file_path": "nguon.rs", "line": 1, "character": 4 }),
    )
    .await;
    assert!(!outcome.is_error, "{outcome:?}");

    let seen = rig.plan.seen.lock().clone();
    assert_eq!(
        seen,
        vec![
            "initialize",
            "initialized",
            "textDocument/didOpen",
            "textDocument/definition",
        ],
        "thứ tự trên dây phải là bắt tay trước, mở tệp, rồi mới hỏi"
    );
}

/// Path <-> URI both ways, with spaces and accented characters; a pure-function test, with the next test covering the same invariant across the whole stack.
#[test]
fn uri_khu_hoi_duoc_voi_khoang_trang_va_tieng_viet() {
    for goc in [
        "/tmp/du an/tệp mã nguồn.rs",
        "/tmp/thư mục/lược đồ.rs",
        "/tmp/Đường Dẫn Có Dấu/tệp #1 (bản sao).rs",
        "/tmp/plain/simple.rs",
    ] {
        let path = PathBuf::from(goc);
        let uri = to_uri(&path).expect("chuyển được sang URI");
        assert!(uri.starts_with("file:///"), "{uri}");
        assert!(
            !uri.contains(' '),
            "khoảng trắng phải được mã hoá, không đi thô: {uri}"
        );
        assert!(
            uri.is_ascii(),
            "URI phải là ASCII sau khi mã hoá theo byte: {uri}"
        );
        assert_eq!(
            from_uri(&uri).expect("giải được URI"),
            path,
            "khứ hồi {goc}"
        );
    }

    // Some shapes must be refused rather than guessed at.
    assert_eq!(
        to_uri(&PathBuf::from("/tmp/a b")).unwrap(),
        "file:///tmp/a%20b"
    );
    assert!(from_uri("https://vi.dụ/x").is_err());
    assert!(
        from_uri("file://may-khac/kho/nguon.rs").is_err(),
        "tệp trên máy khác không được lặng lẽ hoá thành tệp trên máy này"
    );
    assert!(from_uri("file:///tmp/%zz").is_err());
}

/// The same invariant across the whole stack: an accented filename goes out as a URI, comes back, and must decode to exactly the original name.
#[tokio::test]
async fn duong_dan_co_dau_di_tron_vong_qua_server() {
    let ten = "tệp mã nguồn.rs";
    let rig = rig_with(Plan::new(), nhanh(), ten);
    let outcome = goi(
        &rig.tool,
        json!({ "operation": "definition", "file_path": ten, "line": 1, "character": 4 }),
    )
    .await;

    assert!(!outcome.is_error, "{outcome:?}");
    assert!(
        outcome.content.starts_with(&format!("{ten}:1:4")),
        "kết quả phải trỏ về đúng tệp ban đầu, theo đường dẫn tương đối: {}",
        outcome.content
    );
    // The source line comes along, so the model knows what it is looking at before it calls `read`.
    assert!(
        outcome.content.contains("fn xin_chao()"),
        "{}",
        outcome.content
    );
}

/// A server dying mid-flight yields a readable error, and yields it immediately rather than after the query's own sixty-second deadline.
#[tokio::test]
async fn server_chet_giua_chung_thi_bao_loi_chu_khong_treo() {
    let mut plan = Plan::new();
    plan.die_on_query = true;
    let limits = Limits {
        request: Duration::from_secs(30),
        ..nhanh()
    };
    let rig = rig_with(plan, limits, "nguon.rs");

    let bat_dau = std::time::Instant::now();
    let outcome = goi(
        &rig.tool,
        json!({ "operation": "definition", "file_path": "nguon.rs", "line": 1, "character": 4 }),
    )
    .await;

    assert!(outcome.is_error, "{outcome:?}");
    assert!(
        outcome.content.contains("đã dừng"),
        "lỗi phải nói ra rằng server đã chết: {}",
        outcome.content
    );
    assert!(
        bat_dau.elapsed() < Duration::from_secs(5),
        "phải trả lời ngay khi ống đóng, không chờ hết hạn truy vấn: {:?}",
        bat_dau.elapsed()
    );
}

/// A startup timeout must say so rather than return empty - the most important invariant here, because "nothing found" and "cannot answer yet" are different sentences.
#[tokio::test]
async fn het_gio_khi_server_chua_san_sang_thi_noi_ra() {
    let mut plan = Plan::new();
    plan.answer_initialize = false;
    let rig = rig_with(plan, nhanh(), "nguon.rs");

    let outcome = goi(
        &rig.tool,
        json!({ "operation": "definition", "file_path": "nguon.rs", "line": 1, "character": 4 }),
    )
    .await;

    assert!(outcome.is_error, "{outcome:?}");
    assert!(
        outcome.content.contains("chưa khởi động xong"),
        "phải nói rằng server chưa sẵn sàng: {}",
        outcome.content
    );
    assert!(
        !outcome.content.contains("không trả về vị trí nào"),
        "không được để chuyện chưa sẵn sàng đọc như chuyện không tìm thấy: {}",
        outcome.content
    );
}

/// While the server is indexing, every answer carries the notice, non-empty ones included, because a list gathered mid-load is incomplete.
#[tokio::test]
async fn con_dang_lap_chi_muc_thi_ket_qua_noi_rang_no_co_the_thieu() {
    let mut plan = Plan::new();
    plan.stay_busy = true;
    let rig = rig_with(plan, nhanh(), "nguon.rs");

    let outcome = goi(
        &rig.tool,
        json!({ "operation": "references", "file_path": "nguon.rs", "line": 1, "character": 4 }),
    )
    .await;

    assert!(!outcome.is_error, "{outcome:?}");
    assert!(
        outcome.content.contains("còn đang nạp và lập chỉ mục"),
        "{}",
        outcome.content
    );
}

/// Diagnostics are push notifications: they arrive after `didOpen`, and their coordinates must be converted to 1-based like everything else the model reads.
#[tokio::test]
async fn chan_doan_ve_toa_do_1_based() {
    let mut plan = Plan::new();
    plan.publish = Some(json!([{
        "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 12 } },
        "severity": 1,
        "source": "rustc",
        "message": "không tìm thấy macro `println!`",
    }]));
    let rig = rig_with(plan, nhanh(), "nguon.rs");

    let outcome = goi(
        &rig.tool,
        json!({ "operation": "diagnostics", "file_path": "nguon.rs" }),
    )
    .await;

    assert!(!outcome.is_error, "{outcome:?}");
    assert!(
        outcome.content.contains("nguon.rs:2:5 lỗi [rustc]"),
        "0-based của LSP phải thành 1-based của người đọc: {}",
        outcome.content
    );
}

/// `diagnostics` needs no cursor; the other three do, and a missing one must be named, since the model can only fix an argument it is told about.
#[tokio::test]
async fn thieu_con_tro_thi_noi_ro_thieu_gi() {
    let rig = rig_with(Plan::new(), nhanh(), "nguon.rs");
    let call = Invocation::new(
        "lsp".into(),
        "goi-1",
        json!({ "operation": "hover", "file_path": "nguon.rs" })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let err = rig
        .tool
        .execute(&call)
        .await
        .expect_err("phải là lỗi tham số");
    assert!(err.to_string().contains("`line` và `character`"), "{err}");
}

/// Queries outside the working directory are refused exactly where `pai-fs` refuses them.
#[tokio::test]
async fn duong_dan_ngoai_thu_muc_lam_viec_bi_tu_choi() {
    let rig = rig_with(Plan::new(), nhanh(), "nguon.rs");
    let ngoai = rig.root.parent().map(|p| p.join("khong-thuoc-ve-ai.rs"));
    let Some(ngoai) = ngoai else {
        return;
    };
    let call = Invocation::new(
        "lsp".into(),
        "goi-1",
        json!({ "operation": "hover", "file_path": ngoai, "line": 1, "character": 1 })
            .as_object()
            .cloned()
            .unwrap(),
    );
    assert!(
        rig.tool.execute(&call).await.is_err(),
        "tệp ngoài gốc không được đi tới server"
    );
}

/// No detected server means no registered tool - and the other side of the coin, that detection does register one, or this test would pass against a plugin that never registers anything.
#[tokio::test]
async fn khong_do_duoc_server_thi_khong_dang_ky_tool() {
    async fn ten_tool(languages: Vec<LanguageConfig>) -> Vec<String> {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let ctx = Context::root();
        ToolsPlugin.apply(&ctx.plugin("tools")).await.unwrap();
        LspPlugin::new([root.clone()], [], root)
            .with_languages(languages)
            .apply(&ctx.plugin("lsp"))
            .await
            .expect("cắm được plugin kể cả khi không có server nào");
        ctx.require::<Tools>()
            .unwrap()
            .schemas(None)
            .into_iter()
            .map(|schema| schema.name.to_string())
            .collect()
    }

    fn hang(command: &str) -> Vec<LanguageConfig> {
        vec![LanguageConfig {
            id: "rust".into(),
            extensions: vec!["rs".into()],
            command: command.into(),
            args: Vec::new(),
            initialization_options: None,
            enabled: true,
        }]
    }

    let khong_co = ten_tool(hang("khong-he-co-lenh-nay-tren-may-nay-2026")).await;
    assert!(
        !khong_co.iter().any(|name| name == "lsp"),
        "không có language server nào thì tool `lsp` không được có mặt: {khong_co:?}"
    );

    // A command certain to exist and run: the test binary itself.
    let co_that = std::env::current_exe().expect("biết được tệp nhị phân của chính mình");
    let co = ten_tool(hang(&co_that.display().to_string())).await;
    assert!(
        co.iter().any(|name| name == "lsp"),
        "dò được thì phải đăng ký: {co:?}"
    );
}
