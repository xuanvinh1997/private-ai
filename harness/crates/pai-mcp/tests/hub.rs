//! Bất biến của nửa client, kiểm bằng một server MCP giả **trong cùng tiến trình**.
//!
//! Server giả nói MCP thật — cùng bộ mã hoá, cùng cái bắt tay `initialize`, cùng
//! `tools/list` và `tools/call` — chỉ khác chỗ nó chạy trên một [`tokio::io::duplex`] thay
//! vì một socket hay một tiến trình con. Nhờ vậy bài kiểm chứng không chạm mạng, không đẻ
//! tiến trình, không phụ thuộc vào việc máy chạy CI có `npx` hay không, mà vẫn đi qua đúng
//! đoạn mã sẽ chạy thật.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use pai_core::Context;
use pai_mcp::{
    Dialer, DialerFactory, McpHub, McpTransport, Mount, Reach, RetryPolicy, ServerConfig,
    ServerState,
};
use pai_tools::{
    Invocation, Resolution, Tool, ToolMeta, ToolName, ToolOutcome, ToolRegistry, ToolSchema,
    UNTRUSTED_NOTICE,
};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleClient, RoleServer, RunningService, ServiceExt};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

// --- server giả -----------------------------------------------------------------------

fn empty_schema() -> Map<String, Value> {
    json!({ "type": "object", "properties": {} })
        .as_object()
        .cloned()
        .unwrap_or_default()
}

#[derive(Clone)]
struct FakeServer {
    tools: Vec<String>,
    /// Tên tool **đúng như server nhận được**. Đây là thứ chứng minh tiền tố đã được cắt
    /// trước khi gửi đi, chứ không phải được cắt ở đâu đó rồi lại dán vào.
    seen: Arc<Mutex<Vec<String>>>,
}

impl ServerHandler for FakeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .tools
            .iter()
            .map(|name| {
                rmcp::model::Tool::new(
                    name.clone(),
                    format!("tool giả `{name}`"),
                    Arc::new(empty_schema()),
                )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.seen.lock().push(request.name.to_string());
        Ok(CallToolResponse::Complete(CallToolResult::success(vec![
            ContentBlock::text(format!("đã chạy {}", request.name)),
        ])))
    }
}

/// Mở kết nối tới một [`FakeServer`] qua một ống trong bộ nhớ.
struct FakeDialer {
    tools: Vec<String>,
    seen: Arc<Mutex<Vec<String>>>,
    /// Bật lên: mọi lần nối sau đó hỏng, y như một server đã tắt hẳn.
    down: Arc<AtomicBool>,
    /// Huỷ: giết kết nối đang sống.
    kill: CancellationToken,
}

impl FakeDialer {
    fn new(tools: &[&str]) -> FakeDialer {
        FakeDialer {
            tools: tools.iter().map(|s| s.to_string()).collect(),
            seen: Arc::new(Mutex::new(Vec::new())),
            down: Arc::new(AtomicBool::new(false)),
            kill: CancellationToken::new(),
        }
    }
}

#[async_trait]
impl Dialer for FakeDialer {
    async fn dial(&self, ct: CancellationToken) -> anyhow::Result<RunningService<RoleClient, ()>> {
        if self.down.load(Ordering::SeqCst) {
            anyhow::bail!("server giả đang tắt");
        }
        let (client_side, server_side) = tokio::io::duplex(8 * 1024);
        let handler = FakeServer {
            tools: self.tools.clone(),
            seen: self.seen.clone(),
        };
        let server_ct = self.kill.clone();
        tokio::spawn(async move {
            if let Ok(service) = handler.serve_with_ct(server_side, server_ct).await {
                let _ = service.waiting().await;
            }
        });
        Ok(().serve_with_ct(client_side, ct).await?)
    }

    fn reach(&self) -> Reach {
        Reach::InProcess
    }
}

/// Dựng [`FakeDialer`] từ một cấu hình, để bài kiểm chứng đi qua **đúng** đường
/// [`McpHub::reload`] mà người dùng đi, chứ không qua một cửa riêng chỉ test mới dùng.
///
/// Quy ước: đối số của cấu hình chính là danh sách tool mà server giả sẽ công bố.
struct FakeFactory;

impl DialerFactory for FakeFactory {
    fn make(&self, config: &ServerConfig) -> Arc<dyn Dialer> {
        let tools = match &config.transport {
            McpTransport::Stdio { args, .. } => args.clone(),
            McpTransport::Http { .. } => Vec::new(),
        };
        let tools: Vec<&str> = tools.iter().map(String::as_str).collect();
        Arc::new(FakeDialer::new(&tools))
    }
}

fn fake_config(name: &str, tools: &[&str]) -> ServerConfig {
    let mut config = ServerConfig::stdio(name, "gia");
    config.max_retries = 0;
    if let McpTransport::Stdio { args, .. } = &mut config.transport {
        *args = tools.iter().map(|tool| tool.to_string()).collect();
    }
    config
}

// --- một tool nội bộ có thể tự nhận ra mình -------------------------------------------

struct Builtin(ToolName);

#[async_trait]
impl Tool for Builtin {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.0.clone(),
            "tool nội bộ",
            json!({ "type": "object", "properties": {} }),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, pai_tools::ToolError> {
        Ok(ToolOutcome::ok("nội bộ"))
    }
}

// --- tiện ích --------------------------------------------------------------------------

fn setup() -> (Context, Arc<ToolRegistry>, Arc<McpHub>) {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let hub = McpHub::new(registry.clone());
    (ctx, registry, hub)
}

fn fast() -> RetryPolicy {
    RetryPolicy {
        connect_timeout: Duration::from_secs(5),
        max_retries: 0,
    }
}

fn names(registry: &ToolRegistry) -> Vec<String> {
    registry
        .visible(None)
        .into_iter()
        .map(|tool| tool.schema().name.as_str().to_string())
        .collect()
}

fn known(registry: &ToolRegistry, name: &str) -> bool {
    matches!(registry.resolve(None, name), Resolution::Found(_, _))
}

/// Chờ một điều kiện, tối đa hai giây. Trả về `false` nếu hết giờ.
async fn eventually(mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

// --- bài kiểm chứng ---------------------------------------------------------------------

/// Tiền tố được đặt **trước khi** tool vào sổ đăng ký, và cắt ra **trước khi** lời gọi rời máy.
#[tokio::test]
async fn tien_to_dat_truoc_so_dang_ky_va_cat_truoc_khi_gui() {
    let (_ctx, registry, hub) = setup();
    let dialer = Arc::new(FakeDialer::new(&["search"]));
    let seen = dialer.seen.clone();

    assert_eq!(
        hub.mount_dialer("github", dialer, fast()).await,
        Mount::Connected { tools: 1 }
    );

    // Cái sổ đăng ký thấy là cái đã mang tiền tố. Tên trần không tồn tại.
    assert_eq!(names(&registry), vec!["ext.github.search".to_string()]);
    assert!(!known(&registry, "search"));

    let Resolution::Found(tool, name) = registry.resolve(None, "ext.github.search") else {
        panic!("không tra ra tool đã đăng ký");
    };
    let outcome = tool
        .execute(&Invocation::new(name, "c1", Map::new()))
        .await
        .expect("gọi được tool từ xa");
    assert!(outcome.content.contains("đã chạy search"));

    // Cái server thấy là cái tên trần của chính nó — tiền tố đã bị cắt đúng chiều.
    assert_eq!(seen.lock().as_slice(), ["search".to_string()]);
}

/// Một tool bên ngoài trùng tên **không** che được tool nội bộ.
///
/// Đây là bất biến mà cả cái tiền tố tồn tại vì nó. Sổ đăng ký cho đăng ký sau thắng đăng
/// ký trước, nên nếu tên từ xa đi thẳng vào thì một server bên thứ ba công bố `read` sẽ
/// lặng lẽ thay `read` của `pai-fs`.
#[tokio::test]
async fn tool_ngoai_khong_che_duoc_tool_noi_bo() {
    let (_ctx, registry, hub) = setup();
    let keep = registry.register(Arc::new(Builtin(ToolName::new("read"))));

    hub.mount_dialer("srv", Arc::new(FakeDialer::new(&["read"])), fast())
        .await;

    let Resolution::Found(builtin, _) = registry.resolve(None, "read") else {
        panic!("tool nội bộ biến mất");
    };
    assert_eq!(builtin.schema().description, "tool nội bộ");

    let Resolution::Found(remote, _) = registry.resolve(None, "ext.srv.read") else {
        panic!("tool ngoài không được đăng ký");
    };
    assert_eq!(remote.schema().description, "tool giả `read`");

    // Và cả hai cùng tồn tại, chứ không phải cái này thay cái kia.
    assert_eq!(
        names(&registry),
        vec!["ext.srv.read".to_string(), "read".to_string()]
    );
    drop(keep);
}

/// Tool bên ngoài mang đúng giả định xấu nhất, và lời cảnh báo đi vào mô tả mà mô hình đọc.
#[tokio::test]
async fn tool_ngoai_bi_gia_dinh_xau_nhat() {
    let (_ctx, registry, hub) = setup();
    hub.mount_dialer("srv", Arc::new(FakeDialer::new(&["poke"])), fast())
        .await;

    let Resolution::Found(tool, _) = registry.resolve(None, "ext.srv.poke") else {
        panic!("không thấy tool ngoài");
    };
    let meta = tool.meta();
    assert!(
        meta.mutating,
        "tool ngoài phải bị coi là có thay đổi trạng thái"
    );
    assert!(meta.returns_untrusted_content);
    assert!(!meta.concurrency_safe);

    let schema = registry
        .schemas(None)
        .into_iter()
        .find(|s| s.name.as_str() == "ext.srv.poke")
        .expect("schema của tool ngoài");
    assert!(schema.description.contains(UNTRUSTED_NOTICE));
}

/// Một server chết không làm mất tool của server khác, và cũng không làm mất tool nội bộ.
#[tokio::test]
async fn mot_server_chet_khong_lam_mat_tool_cua_ai_khac() {
    let (_ctx, registry, hub) = setup();
    let keep = registry.register(Arc::new(Builtin(ToolName::new("bash"))));

    let alpha = Arc::new(FakeDialer::new(&["a1"]));
    let beta = Arc::new(FakeDialer::new(&["b1"]));
    hub.mount_dialer("alpha", alpha.clone(), fast()).await;
    hub.mount_dialer("beta", beta.clone(), fast()).await;
    assert!(known(&registry, "ext.alpha.a1"));
    assert!(known(&registry, "ext.beta.b1"));

    // Giết alpha và không cho nó dựng lại được.
    alpha.down.store(true, Ordering::SeqCst);
    alpha.kill.cancel();

    assert!(
        eventually(|| !known(&registry, "ext.alpha.a1")).await,
        "tool của server đã chết phải biến khỏi sổ đăng ký"
    );
    assert!(
        eventually(|| matches!(hub.state("alpha"), Some(ServerState::Failed { .. }))).await,
        "trạng thái của alpha phải nói ra rằng nó hỏng"
    );

    // Và mọi thứ khác vẫn nguyên.
    assert!(known(&registry, "ext.beta.b1"));
    assert!(known(&registry, "bash"));
    drop(keep);
}

/// Một server không nối được ngay từ đầu cũng không kéo theo ai.
#[tokio::test]
async fn server_khong_noi_duoc_van_la_ok() {
    let (_ctx, registry, hub) = setup();
    let keep = registry.register(Arc::new(Builtin(ToolName::new("read"))));

    let broken = Arc::new(FakeDialer::new(&["x"]));
    broken.down.store(true, Ordering::SeqCst);

    let mount = hub.mount_dialer("hỏng", broken, fast()).await;
    assert!(matches!(mount, Mount::Unavailable { .. }));
    assert!(known(&registry, "read"));
    assert_eq!(names(&registry), vec!["read".to_string()]);
    drop(keep);
}

/// Thêm và bớt một server không cần khởi động lại gì cả, và không đụng tới server đang khoẻ.
#[tokio::test]
async fn hot_reload_khong_dung_toi_server_khong_doi() {
    let (_ctx, registry, hub) = setup();
    hub.mount_dialer("alpha", Arc::new(FakeDialer::new(&["a1"])), fast())
        .await;
    let beta = Arc::new(FakeDialer::new(&["b1"]));
    hub.mount_dialer("beta", beta.clone(), fast()).await;

    // Bớt một cái.
    assert!(hub.unmount("alpha").await);
    assert!(!known(&registry, "ext.alpha.a1"));
    assert!(known(&registry, "ext.beta.b1"), "beta không được đụng tới");
    assert_eq!(hub.servers(), vec!["beta".to_string()]);

    // Thêm một cái mới, ngay lúc đang chạy.
    hub.mount_dialer("gamma", Arc::new(FakeDialer::new(&["g1"])), fast())
        .await;
    assert!(known(&registry, "ext.gamma.g1"));
    assert!(known(&registry, "ext.beta.b1"));

    // Gỡ hết: sổ đăng ký sạch, không còn dấu vết nào phải dọn bằng tay.
    hub.shutdown().await;
    assert!(names(&registry).is_empty());
    assert!(hub.servers().is_empty());
}

/// Cắm đè lên một cái tên đang có là **thay thế**, không phải nhân đôi.
#[tokio::test]
async fn cam_de_len_mot_ten_dang_co_la_thay_the() {
    let (_ctx, registry, hub) = setup();
    hub.mount_dialer("srv", Arc::new(FakeDialer::new(&["cũ"])), fast())
        .await;
    hub.mount_dialer("srv", Arc::new(FakeDialer::new(&["mới"])), fast())
        .await;

    assert_eq!(names(&registry), vec!["ext.srv.mới".to_string()]);
    assert_eq!(hub.servers(), vec!["srv".to_string()]);
}

/// `reload` chỉ đụng vào cái đã đổi.
///
/// Bài này chạy trên transport thật (một lệnh không tồn tại), vì thứ nó khoá là **phép so
/// cấu hình**, không phải phép nối. Một server hỏng vẫn nằm trong danh sách — người dùng
/// phải thấy được là nó hỏng — nên nó là chỗ tốt để hỏi "lần reload này có đụng vào nó
/// không".
#[tokio::test]
async fn reload_chi_dung_vao_cai_da_doi() {
    let (_ctx, _registry, hub) = setup();
    let mut config = ServerConfig::stdio("dead", "/khong-co-lenh-nay-dau-2f8a");
    config.max_retries = 0;
    config.connect_timeout_secs = 2;

    let mount = hub.mount(config.clone()).await.expect("cấu hình hợp lệ");
    assert!(matches!(mount, Mount::Unavailable { .. }));
    assert_eq!(hub.servers(), vec!["dead".to_string()]);

    // Cấu hình y hệt: không cắm lại, không báo cáo gì cả.
    assert!(hub.reload(vec![config.clone()]).await.is_empty());
    assert_eq!(hub.servers(), vec!["dead".to_string()]);

    // Đổi một chi tiết: cắm lại.
    let mut changed = config.clone();
    if let McpTransport::Stdio { args, .. } = &mut changed.transport {
        args.push("--khac".to_string());
    }
    assert_eq!(hub.reload(vec![changed]).await.len(), 1);
    assert_eq!(hub.servers(), vec!["dead".to_string()]);

    // Bỏ khỏi danh sách: gỡ hẳn.
    assert!(hub.reload(Vec::new()).await.is_empty());
    assert!(hub.servers().is_empty());

    // Cấu hình sai được báo cáo chứ không làm hỏng cả lượt reload.
    let report = hub.reload(vec![ServerConfig::stdio("a.b", "x")]).await;
    assert_eq!(report.len(), 1);
    assert!(report[0].1.is_err());
    assert!(hub.servers().is_empty());
}

/// `status()` hiện đúng cái tên mà mô hình gọi — tên **đã mang tiền tố**.
///
/// Hub biết tên trần vì đó là cái server công bố; giao diện phải hiện tên đầy đủ vì đó là
/// cái duy nhất tra ra được trong sổ đăng ký. Lẫn hai thứ đó thì người dùng đọc một cái
/// tên rồi đi tìm một tool không tồn tại.
#[tokio::test]
async fn status_tra_ten_tool_da_mang_tien_to() {
    let (_ctx, _registry, hub) = setup();
    hub.mount_dialer(
        "github",
        Arc::new(FakeDialer::new(&["search", "issues"])),
        fast(),
    )
    .await;

    let status = hub.status();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].name, "github");
    assert_eq!(status[0].state, ServerState::Ready { tools: 2 });
    assert_eq!(
        status[0].tools,
        vec![
            "ext.github.search".to_string(),
            "ext.github.issues".to_string()
        ]
    );
    assert!(
        status[0].error.is_none(),
        "chưa hỏng lần nào thì chưa có lý do"
    );

    // Một server không nối được nói ra lý do, và giữ lại lý do đó.
    let broken = Arc::new(FakeDialer::new(&["x"]));
    broken.down.store(true, Ordering::SeqCst);
    hub.mount_dialer("hong", broken, fast()).await;

    let status = hub.status();
    assert_eq!(
        status.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["github", "hong"],
        "danh sách phải xếp theo tên để giao diện không nhảy hàng"
    );
    assert!(status[1].tools.is_empty());
    let reason = status[1]
        .error
        .clone()
        .expect("server hỏng phải nói vì sao");
    assert!(
        reason.contains("server giả đang tắt"),
        "lý do thật: {reason}"
    );
}

/// Tắt một server bằng `reload` **không** đụng tới tool của server khác.
///
/// Khẳng định bằng danh sách tool thật trong sổ đăng ký, vì đó là thứ mô hình nhìn thấy;
/// một `state` nói "Ready" trong khi sổ đăng ký đã bị dọn sạch vẫn là bài kiểm chứng xanh
/// và một ứng dụng hỏng.
#[tokio::test]
async fn tat_mot_server_khong_lam_dut_tool_cua_server_khac() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let hub = McpHub::with_dialers(registry.clone(), Arc::new(FakeFactory));

    let alpha = fake_config("alpha", &["a1"]);
    let beta = fake_config("beta", &["b1", "b2"]);

    hub.reload(vec![alpha.clone(), beta.clone()]).await;
    assert_eq!(
        names(&registry),
        vec![
            "ext.alpha.a1".to_string(),
            "ext.beta.b1".to_string(),
            "ext.beta.b2".to_string()
        ]
    );

    let mut tat = alpha.clone();
    tat.enabled = false;
    hub.reload(vec![tat, beta]).await;

    assert_eq!(
        names(&registry),
        vec!["ext.beta.b1".to_string(), "ext.beta.b2".to_string()],
        "chỉ alpha được gỡ, beta không được đụng tới"
    );
    assert_eq!(hub.servers(), vec!["beta".to_string()]);
    let status = hub.status();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].tools, vec!["ext.beta.b1", "ext.beta.b2"]);
}
