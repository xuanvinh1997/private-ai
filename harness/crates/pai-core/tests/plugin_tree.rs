//! Kiểm chứng lõi bằng đúng hình dạng mà harness sẽ dùng: một seam, một plugin đóng góp
//! vào seam đó, và một middleware chen vào giữa để phủ quyết.
//!
//! Nếu ba thứ này không ghép được với nhau thì mọi thứ xây bên trên đều sai, nên bài này
//! chạy trước khi có tool thật.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Notify, ServiceKey, Waterfall};

// --- một seam -----------------------------------------------------------------------

#[async_trait]
trait ToolRegistry: Send + Sync {
    async fn call(&self, name: &str, args: &str) -> String;
}

enum Tools {}
impl ServiceKey for Tools {
    type Api = dyn ToolRegistry;
    const NAME: &'static str = "tools";
}

struct EchoTools;

#[async_trait]
impl ToolRegistry for EchoTools {
    async fn call(&self, name: &str, args: &str) -> String {
        format!("{name}({args})")
    }
}

// --- một waterfall ------------------------------------------------------------------

struct ToolCall {
    name: String,
    args: String,
}

enum PreExecute {}
impl Waterfall for PreExecute {
    const NAME: &'static str = "tools/pre-execute";
    type Req = ToolCall;
    type Out = Result<String, String>;
}

/// Chặn một tool và không uỷ quyền — đúng nghĩa phủ quyết.
struct DenyGate {
    deny: &'static str,
}

impl Middleware<PreExecute> for DenyGate {
    fn call<'a>(
        &'a self,
        req: &'a mut ToolCall,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, Result<String, String>> {
        async move {
            if req.name == self.deny {
                // Từ chối là văn bản, không phải lỗi: mô hình phải đọc được vì sao.
                return Err(format!(
                    "tool `{}` không được phép trong phạm vi này",
                    req.name
                ));
            }
            next.run(req).await
        }
        .boxed()
    }
}

/// Sửa yêu cầu rồi vẫn uỷ quyền — nhánh cộng tác, khác hẳn nhánh phủ quyết.
struct Rewrite;

impl Middleware<PreExecute> for Rewrite {
    fn call<'a>(
        &'a self,
        req: &'a mut ToolCall,
        next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, Result<String, String>> {
        async move {
            req.args = format!("{}+đã-sửa", req.args);
            next.run(req).await
        }
        .boxed()
    }
}

// --- một sự kiện quan sát -----------------------------------------------------------

enum ToolCalled {}
impl Notify for ToolCalled {
    const NAME: &'static str = "tool/call";
    type Payload = String;
}

// --- ráp lại ------------------------------------------------------------------------

async fn dispatch(ctx: &Context, name: &str, args: &str) -> Result<String, String> {
    let mut req = ToolCall {
        name: name.into(),
        args: args.into(),
    };
    ctx.waterfall::<PreExecute, _>(&mut req, |req| {
        let ctx = ctx.clone();
        async move {
            let tools = ctx.require::<Tools>().map_err(|e| e.to_string())?;
            Ok(tools.call(&req.name, &req.args).await)
        }
        .boxed()
    })
    .await
}

#[tokio::test]
async fn seam_middleware_va_effect_ghep_duoc_voi_nhau() {
    let root = Context::root();

    // Plugin cung cấp provider cho seam.
    let tools_plugin = root.plugin("tools");
    let provided = tools_plugin
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("cắm được");
    tools_plugin.keep(provided);

    // Không có middleware nào: chạy thẳng vào thân.
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs)".into())
    );

    // Một plugin chính sách chen vào. Nó không biết gì về `EchoTools`.
    let gate = root.plugin("gate");
    let gate_guard = gate.on_waterfall::<PreExecute>(Arc::new(DenyGate { deny: "bash" }));
    gate.keep(gate_guard);

    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs)".into())
    );
    assert!(dispatch(&root, "bash", "rm -rf /").await.is_err());

    // Một plugin nữa sửa yêu cầu rồi vẫn uỷ quyền.
    let rewrite = root.plugin("rewrite");
    let rewrite_guard = rewrite.on_waterfall::<PreExecute>(Arc::new(Rewrite));
    rewrite.keep(rewrite_guard);
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs+đã-sửa)".into())
    );

    // Gỡ plugin chính sách: đăng ký của nó biến mất, phần còn lại nguyên vẹn.
    gate.effects().dispose().await;
    assert!(dispatch(&root, "bash", "ls").await.is_ok());
    assert_eq!(
        dispatch(&root, "read", "a.rs").await,
        Ok("read(a.rs+đã-sửa)".into())
    );
}

#[tokio::test]
async fn go_plugin_la_thu_hoi_dang_ky_cua_no() {
    let root = Context::root();
    let plugin = root.plugin("tools");
    let guard = plugin
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("cắm được");
    plugin.keep(guard);

    assert!(root.get::<Tools>().is_some());
    plugin.effects().dispose().await;
    assert!(root.get::<Tools>().is_none());
}

#[tokio::test]
async fn hai_provider_cung_seam_cung_coi_la_loi_cau_hinh() {
    let root = Context::root();
    let first = root
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("cắm được");
    assert!(root.provide::<Tools>(Arc::new(EchoTools)).is_err());

    // Nhưng ở một cõi khác thì sống cạnh nhau được.
    let isolated = root.isolate::<Tools>();
    let second = isolated
        .provide::<Tools>(Arc::new(EchoTools))
        .expect("cõi riêng");

    first.leak();
    second.leak();
}

#[tokio::test]
async fn cho_service_xuat_hien_thay_vi_sap_xep_thu_tu_khoi_dong() {
    let root = Context::root();
    let waiter = root.clone();
    let task = tokio::spawn(async move { waiter.wait_for::<Tools>().await.call("x", "").await });

    // Cắm muộn. Bên chờ phải tự thức dậy.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    root.provide::<Tools>(Arc::new(EchoTools))
        .expect("cắm được")
        .leak();

    assert_eq!(task.await.unwrap(), "x()");
}

#[tokio::test]
async fn listener_co_pham_vi_khong_cham_toi_agent_khac() {
    let root = Context::root();
    let seen = Arc::new(AtomicUsize::new(0));

    let agent = root.scoped("agent-a");
    let counter = seen.clone();
    let guard = agent.on_notify::<ToolCalled>(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    agent.keep(guard);

    // Phát ở gốc: listener của agent con không nhận, vì sự kiện chảy lên chứ không xuống.
    root.notify::<ToolCalled>(&"read".to_string());
    assert_eq!(seen.load(Ordering::SeqCst), 0);

    // Phát trong phạm vi của chính nó: nhận.
    agent.notify::<ToolCalled>(&"read".to_string());
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}
