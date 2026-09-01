//! Bản đặc tả bảo mật của crate, port từ `tests/test_mcp.py` của bản Python.
//!
//! Bốn bài đầu là bốn bất biến mà bản Python làm đúng hơn dsh, nên chúng được chép sang
//! trước khi có tool thật nào. Ba bài sau là những thứ chỉ bản Rust mới hứa được: canh
//! gác đơn điệu, phê duyệt fail-closed, và output dài không bị cắt mất.
//!
//! Đọc chúng như đọc luật, không như đọc test: mỗi bài khoá đúng một câu, và câu đó viết
//! ở dòng doc của bài.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, ScopeKey};
use pai_tools::builtin::TodoWrite;
use pai_tools::pipeline::{
    ApprovalRequest, Approver, PreDecision, PreExecute, PreRequest, ToolGuard, ToolPipeline,
    not_available,
};
use pai_tools::schema::UNTRUSTED_NOTICE;
use pai_tools::seam::{Approval, Spill};
use pai_tools::spill::{MemorySpillStore, SpillRef, SpillStore};
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolName, ToolOutcome, ToolRegistry, ToolRestriction,
    ToolSchema,
};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

// --- một tool giả có thể chứng minh nó đã chạy hay chưa ------------------------------

struct Fake {
    name: ToolName,
    meta: ToolMeta,
    parameters: Value,
    /// Đếm số lần **thân tool** thật sự chạy. Đây là thứ chứng minh một lời từ chối xảy
    /// ra trước khi chạm vào tool, chứ không phải sau khi đã lỡ làm gì đó.
    ran: Arc<AtomicUsize>,
    /// Tham số mà thân tool nhìn thấy — sau ghim, sau mọi middleware.
    seen: Arc<Mutex<Map<String, Value>>>,
}

impl Fake {
    fn new(name: &str, meta: ToolMeta) -> Fake {
        Fake {
            name: ToolName::new(name),
            meta,
            parameters: json!({ "type": "object", "properties": {}, "required": [] }),
            ran: Arc::new(AtomicUsize::new(0)),
            seen: Arc::new(Mutex::new(Map::new())),
        }
    }

    fn with_parameters(mut self, parameters: Value) -> Fake {
        self.parameters = parameters;
        self
    }
}

#[async_trait]
impl Tool for Fake {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.name.clone(),
            format!("tool giả `{}`", self.name),
            self.parameters.clone(),
        )
    }

    fn meta(&self) -> ToolMeta {
        self.meta.clone()
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock() = call.arguments.clone();
        Ok(ToolOutcome::ok(format!("{} đã chạy", self.name)))
    }
}

/// Bốn tool đủ để diễn tả ranh giới chỉ-đọc: hai cái nhìn, hai cái đụng.
fn library() -> Vec<Arc<Fake>> {
    vec![
        Arc::new(Fake::new("workspaces.list", ToolMeta::read_only())),
        Arc::new(Fake::new("documents.list", ToolMeta::read_only())),
        Arc::new(Fake::new("documents.ingest_text", ToolMeta::mutating())),
        Arc::new(Fake::new("documents.delete", ToolMeta::mutating())),
    ]
}

/// Dựng một agent có phạm vi, đã cắm sẵn thư viện tool ở tầng toàn cục.
fn bench() -> (
    Context,
    Context,
    Arc<ToolRegistry>,
    ScopeKey,
    Vec<Arc<Fake>>,
) {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    let tools = library();
    for tool in &tools {
        root.keep(registry.register(tool.clone()));
    }
    let agent = root.scoped("agent");
    let scope = agent
        .scope_key()
        .expect("ngữ cảnh có phạm vi thì có khoá phạm vi");
    (root, agent, registry, scope, tools)
}

fn names(schemas: &[ToolSchema]) -> Vec<String> {
    schemas
        .iter()
        .map(|s| s.name.as_str().to_string())
        .collect()
}

// --- 1. tập cho phép đúng bằng tập tool không thay đổi gì -----------------------------

/// Port của `test_the_allow_set_is_exactly_the_non_mutating_tools` +
/// `test_a_mutating_tool_is_never_advertised_to_the_agent`.
///
/// Khoá: **cái được quảng cáo cho một agent chỉ-đọc đúng bằng tập tool có
/// `mutating == false`, không hơn một cái nào.**
#[tokio::test]
async fn tap_quang_cao_dung_bang_tap_tool_khong_thay_doi_gi() {
    let (_root, _agent, registry, scope, _tools) = bench();

    let everything = registry.visible(None);
    let read_only: HashSet<ToolName> = everything
        .iter()
        .filter(|tool| !tool.meta().mutating)
        .map(|tool| tool.schema().name)
        .collect();
    let mutating: HashSet<ToolName> = everything
        .iter()
        .filter(|tool| tool.meta().mutating)
        .map(|tool| tool.schema().name)
        .collect();

    // Hai tập rời nhau — chính là bất biến mà `READ_ONLY_TOOLS` tồn tại để giữ.
    assert!(read_only.is_disjoint(&mutating));
    assert_eq!(read_only.len(), 2);
    assert_eq!(mutating.len(), 2);

    let guard = registry.restrict(
        scope,
        ToolRestriction::allow_only(read_only.iter().cloned()),
    );

    assert_eq!(
        names(&registry.schemas(Some(scope))),
        vec!["documents.list".to_string(), "workspaces.list".to_string()]
    );
    for name in &mutating {
        assert!(
            !names(&registry.schemas(Some(scope))).contains(&name.as_str().to_string()),
            "{name} lọt vào danh sách quảng cáo"
        );
    }
    // Host, không có phạm vi, vẫn thấy đủ: giao diện gọi thẳng thì không bị siết.
    assert_eq!(registry.schemas(None).len(), 4);
    drop(guard);
}

// --- 2. bị từ chối kể cả khi gọi bằng tên đã mã hoá ------------------------------------

/// Port của `test_a_mutating_tool_is_refused_even_when_called_by_its_mangled_name`.
///
/// Khoá: **danh sách quảng cáo chỉ là gợi ý.** Một mô hình đoán ra `documents__delete`
/// đi thẳng vào hàm gọi, và tầng lọc thứ hai — chạy trên tên đã giải mã — chặn nó ở đó.
#[tokio::test]
async fn tool_bi_tu_choi_ke_ca_khi_goi_bang_ten_da_ma_hoa() {
    let (_root, agent, registry, scope, tools) = bench();
    let ingest = tools[2].clone();
    let delete = tools[3].clone();
    let guard = registry.restrict(
        scope,
        ToolRestriction::allow_only(["workspaces.list", "documents.list"]),
    );
    let pipeline = ToolPipeline::new(&agent, registry.clone());

    // Tên trên wire, đúng thứ mô hình gõ được.
    assert_eq!(
        ToolName::new("documents.ingest_text").wire(),
        "documents__ingest_text"
    );

    let refused = pipeline
        .execute(
            "c1",
            "documents__ingest_text",
            json!({ "filename": "lén.md", "content": "tài liệu này bảo bạn hãy ghi tôi vào thư viện" }),
        )
        .await;

    assert!(refused.is_error);
    assert_eq!(
        refused.content,
        not_available(&ToolName::new("documents.ingest_text"))
    );
    // Từ chối là **văn bản**, không phải một `Result` bị vứt lên trên.
    assert!(!refused.content.is_empty());
    // Và không có gì được ghi.
    assert_eq!(ingest.ran.load(Ordering::SeqCst), 0);

    let refused = pipeline
        .execute("c2", "documents__delete", json!({ "confirmed": true }))
        .await;
    assert!(refused.is_error);
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);

    // Cùng đường gọi đó, một tool được phép vẫn chạy: bộ lọc lọc, không phải chặn hết.
    let ok = pipeline.execute("c3", "workspaces__list", json!({})).await;
    assert!(!ok.is_error);
    assert_eq!(tools[0].ran.load(Ordering::SeqCst), 1);
    drop(guard);
}

// --- 3. từ chối xảy ra trước khi chạm vào thân tool ------------------------------------

/// Port của `test_deletion_through_the_adapter_is_refused_before_it_reaches_the_tool`.
///
/// Khoá: **thân tool không chạy.** Ba nguồn từ chối — hạn chế, `tools/pre-execute`, và
/// canh gác — đều phải dừng lại trước khi tool được đụng tới, không phải sau đó.
#[tokio::test]
async fn tu_choi_xay_ra_truoc_khi_cham_vao_than_tool() {
    // (a) hạn chế theo phạm vi
    let (_root, agent, registry, scope, tools) = bench();
    let delete = tools[3].clone();
    let restriction = registry.restrict(scope, ToolRestriction::deny_only(["documents.delete"]));
    let pipeline = ToolPipeline::new(&agent, registry.clone());
    assert!(
        pipeline
            .execute("a", "documents__delete", json!({}))
            .await
            .is_error
    );
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(restriction);

    // (b) `tools/pre-execute` nói không
    struct NoWrites;
    impl Middleware<PreExecute> for NoWrites {
        fn call<'a>(
            &'a self,
            req: &'a mut PreRequest,
            next: Next<'a, PreExecute>,
        ) -> BoxFuture<'a, PreDecision> {
            async move {
                if req.meta.mutating {
                    return PreDecision::Deny(format!(
                        "`{}` thay đổi trạng thái; lượt này chỉ được đọc.",
                        req.name
                    ));
                }
                next.run(req).await
            }
            .boxed()
        }
    }
    let hook = agent.on_waterfall::<PreExecute>(Arc::new(NoWrites));
    let denied = pipeline.execute("b", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert!(denied.content.contains("chỉ được đọc"));
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(hook);

    // (c) canh gác
    let gate = registry.add_guard(Some(scope), Arc::new(DenyAll));
    let denied = pipeline.execute("c", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert_eq!(denied.content, "canh gác nói không");
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(gate);

    // Gỡ hết chính sách thì tool chạy — nghĩa là ba bài trên đo chính sách, không đo một
    // tool hỏng sẵn.
    assert!(
        !pipeline
            .execute("d", "documents__delete", json!({}))
            .await
            .is_error
    );
    assert_eq!(delete.ran.load(Ordering::SeqCst), 1);
}

// --- 4. tham số ghim biến mất khỏi schema và bị ghi đè lúc gọi ------------------------

/// Port của `_bind_workspace` + `invoker` (`adapter.py:77-93`, `:150-156`).
///
/// Khoá: **tham số mô hình không thấy là tham số nó không thể làm sai.** `workspace_id`
/// biến mất khỏi schema, và giá trị mô hình tự gửi bị **ghi đè** chứ không được dùng làm
/// mặc định — kể cả khi một middleware ở `tools/pre-execute` cố đặt lại nó.
#[tokio::test]
async fn tham_so_ghim_bien_mat_khoi_schema_va_bi_ghi_de() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    let search = Arc::new(
        Fake::new("rag.vector.search", ToolMeta::read_only()).with_parameters(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "workspace_id": { "type": "string" },
                "limit": { "type": "integer" }
            },
            "required": ["query", "workspace_id"]
        })),
    );
    root.keep(registry.register(search.clone()));
    let agent = root.scoped("agent");
    let scope = agent.scope_key().expect("có phạm vi");

    // Chưa ghim: mô hình thấy đủ ba tham số, đúng như bản Python quảng cáo.
    let before = registry.schemas(Some(scope));
    let props = before[0].parameters["properties"]
        .as_object()
        .expect("object schema");
    assert!(props.contains_key("query") && props.contains_key("workspace_id"));

    let pin = registry.pin(scope, "workspace_id", json!("ws-cua-toi"));

    let after = registry.schemas(Some(scope));
    let props = after[0].parameters["properties"]
        .as_object()
        .expect("object schema");
    assert!(
        !props.contains_key("workspace_id"),
        "tham số ghim vẫn còn trong schema"
    );
    assert!(props.contains_key("query"), "ghim làm mất cả tham số khác");
    let required: Vec<&str> = after[0].parameters["required"]
        .as_array()
        .expect("mảng")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(required, vec!["query"]);

    // Một middleware "vô hại" cố đặt lại workspace_id — ghim vẫn phải thắng, nếu không
    // thì hook trở thành đường vòng qua đúng cái ràng buộc này.
    struct Meddle;
    impl Middleware<PreExecute> for Meddle {
        fn call<'a>(
            &'a self,
            req: &'a mut PreRequest,
            next: Next<'a, PreExecute>,
        ) -> BoxFuture<'a, PreDecision> {
            async move {
                req.arguments
                    .insert("workspace_id".into(), json!("ws-cua-hook"));
                next.run(req).await
            }
            .boxed()
        }
    }
    let hook = agent.on_waterfall::<PreExecute>(Arc::new(Meddle));

    let pipeline = ToolPipeline::new(&agent, registry.clone());
    let outcome = pipeline
        .execute(
            "c1",
            "rag__vector__search",
            json!({ "query": "bí mật", "workspace_id": "ws-cua-nguoi-khac" }),
        )
        .await;

    assert!(!outcome.is_error);
    assert_eq!(
        search.seen.lock().get("workspace_id"),
        Some(&json!("ws-cua-toi"))
    );
    assert_eq!(search.seen.lock().get("query"), Some(&json!("bí mật")));
    drop(hook);
    drop(pin);
}

// --- 5. canh gác đơn điệu không đảo ngược được ----------------------------------------

struct DenyAll;

#[async_trait]
impl ToolGuard for DenyAll {
    fn name(&self) -> &'static str {
        "deny-all"
    }
    async fn check(&self, _call: &Invocation, _meta: &ToolMeta) -> Option<String> {
        Some("canh gác nói không".into())
    }
}

/// Canh gác "dễ tính" nhất mà trait cho phép viết: nó vẫn chỉ bỏ qua được.
struct Abstain;

#[async_trait]
impl ToolGuard for Abstain {
    fn name(&self) -> &'static str {
        "abstain"
    }
    async fn check(&self, _call: &Invocation, _meta: &ToolMeta) -> Option<String> {
        None
    }
}

/// Khoá: **thứ tự đăng ký canh gác không đổi được câu trả lời.** Không có nhánh cho phép
/// trong [`ToolGuard`], nên không canh gác nào — đăng ký trước hay sau — gỡ được lời từ
/// chối của canh gác khác.
#[tokio::test]
async fn canh_gac_don_dieu_khong_dao_nguoc_duoc() {
    for order in [0, 1] {
        let (_root, agent, registry, scope, tools) = bench();
        let list = tools[1].clone();
        let guards = if order == 0 {
            vec![
                registry.add_guard(Some(scope), Arc::new(Abstain)),
                registry.add_guard(Some(scope), Arc::new(DenyAll)),
            ]
        } else {
            vec![
                registry.add_guard(Some(scope), Arc::new(DenyAll)),
                registry.add_guard(Some(scope), Arc::new(Abstain)),
            ]
        };

        let pipeline = ToolPipeline::new(&agent, registry.clone());
        let outcome = pipeline.execute("c1", "documents__list", json!({})).await;

        assert!(outcome.is_error, "thứ tự {order} lọt qua canh gác");
        assert_eq!(outcome.content, "canh gác nói không");
        assert_eq!(list.ran.load(Ordering::SeqCst), 0);
        drop(guards);
    }
}

// --- 6. phê duyệt fail-closed ---------------------------------------------------------

struct AlwaysAsk;

impl Middleware<PreExecute> for AlwaysAsk {
    fn call<'a>(
        &'a self,
        req: &'a mut PreRequest,
        _next: Next<'a, PreExecute>,
    ) -> BoxFuture<'a, PreDecision> {
        let name = req.name.clone();
        async move {
            PreDecision::Ask {
                reason: format!("`{name}` cần người dùng đồng ý"),
            }
        }
        .boxed()
    }
}

struct Says(bool);

#[async_trait]
impl Approver for Says {
    async fn approve(&self, _request: &ApprovalRequest) -> bool {
        self.0
    }
}

struct NeverAnswers;

#[async_trait]
impl Approver for NeverAnswers {
    async fn approve(&self, _request: &ApprovalRequest) -> bool {
        // Một hộp thoại bị che sau cửa sổ khác. Không bao giờ trả lời.
        std::future::pending::<()>().await;
        true
    }
}

/// Khoá: **không có approver, hoặc hết giờ, đều là từ chối.** Không có nhánh nào trong
/// đường ống biến "không hỏi được" thành "cho chạy".
#[tokio::test]
async fn khong_co_approver_hoac_het_gio_deu_la_tu_choi() {
    let (root, agent, registry, _scope, tools) = bench();
    let delete = tools[3].clone();
    let ask = agent.on_waterfall::<PreExecute>(Arc::new(AlwaysAsk));
    let pipeline = ToolPipeline::new(&agent, registry.clone())
        .with_approval_timeout(std::time::Duration::from_millis(50));

    // (a) không ai để hỏi
    let denied = pipeline.execute("a", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert!(denied.content.contains("không cho phép"));
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);

    // (b) có người hỏi nhưng không bao giờ trả lời
    let hung: Arc<dyn Approver> = Arc::new(NeverAnswers);
    let mounted = root.provide::<Approval>(hung).expect("cắm được");
    let denied = pipeline.execute("b", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(mounted);

    // (c) người dùng nói không
    let no: Arc<dyn Approver> = Arc::new(Says(false));
    let mounted = root.provide::<Approval>(no).expect("cắm được");
    assert!(
        pipeline
            .execute("c", "documents__delete", json!({}))
            .await
            .is_error
    );
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(mounted);

    // (d) người dùng nói có — và chỉ lúc đó tool mới chạy
    let yes: Arc<dyn Approver> = Arc::new(Says(true));
    let mounted = root.provide::<Approval>(yes).expect("cắm được");
    assert!(
        !pipeline
            .execute("d", "documents__delete", json!({}))
            .await
            .is_error
    );
    assert_eq!(delete.ran.load(Ordering::SeqCst), 1);
    drop(mounted);
    drop(ask);
}

/// Khoá: **phê duyệt không mở được cái mà canh gác đã đóng.** Canh gác chạy sau, nên một
/// lần bấm "cho phép" không thể vượt qua chính sách của chủ sở hữu.
#[tokio::test]
async fn phe_duyet_khong_mo_duoc_cai_canh_gac_da_dong() {
    let (root, agent, registry, scope, tools) = bench();
    let ask = agent.on_waterfall::<PreExecute>(Arc::new(AlwaysAsk));
    let gate = registry.add_guard(Some(scope), Arc::new(DenyAll));
    let yes: Arc<dyn Approver> = Arc::new(Says(true));
    let mounted = root.provide::<Approval>(yes).expect("cắm được");

    let pipeline = ToolPipeline::new(&agent, registry.clone());
    let outcome = pipeline.execute("c1", "documents__delete", json!({})).await;

    assert!(outcome.is_error);
    assert_eq!(outcome.content, "canh gác nói không");
    assert_eq!(tools[3].ran.load(Ordering::SeqCst), 0);
    drop(mounted);
    drop(gate);
    drop(ask);
}

// --- 7. ranh giới tin cậy và kho tràn -------------------------------------------------

/// Port của `test_every_retrieval_tool_repeats_the_untrusted_framing`.
///
/// Khoá: **lời cảnh báo nằm trong mô tả tool**, và nó được chèn bởi sổ đăng ký chứ không
/// bởi tác giả tool — một luật mà mỗi người phải nhớ áp dụng là một luật sẽ có chỗ quên.
#[tokio::test]
async fn tool_tra_noi_dung_khong_dang_tin_cay_tu_mang_canh_bao_trong_mo_ta() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    root.keep(registry.register(Arc::new(Fake::new(
        "rag.web.search",
        ToolMeta::read_only().untrusted(),
    ))));
    root.keep(registry.register(Arc::new(Fake::new("system.time", ToolMeta::read_only()))));

    let schemas = registry.schemas(None);
    let web = schemas
        .iter()
        .find(|s| s.name.as_str() == "rag.web.search")
        .expect("có");
    let time = schemas
        .iter()
        .find(|s| s.name.as_str() == "system.time")
        .expect("có");

    assert!(web.description.contains(UNTRUSTED_NOTICE));
    assert!(
        web.description.contains("tool giả"),
        "mô tả gốc bị nuốt mất"
    );
    // Không dán bừa lên tool không trả nội dung ngoài: cảnh báo ở khắp nơi là cảnh báo ở
    // không đâu cả.
    assert!(!time.description.contains(UNTRUSTED_NOTICE));

    // Và metadata của host không đi kèm ra mô hình: schema đúng ba trường.
    let wire = serde_json::to_value(web).expect("serialize được");
    let fields: HashSet<&str> = wire
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(fields, HashSet::from(["name", "description", "parameters"]));
    assert_eq!(wire["name"], json!("rag__web__search"));
}

struct Verbose;

#[async_trait]
impl Tool for Verbose {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "files.read",
            "đọc một tệp",
            json!({ "type": "object", "properties": {} }),
        )
    }
    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }
    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::ok("x".repeat(5_000)))
    }
}

/// Khoá: **output dài được giữ nguyên vẹn.** Bản Python cắt ở 6000 ký tự và mất phần dư;
/// ở đây ngưỡng chỉ quyết định mô hình đọc bao nhiêu, không quyết định cái gì còn tồn tại.
#[tokio::test]
async fn output_dai_duoc_cat_vao_kho_chu_khong_bi_cat_cut() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    root.keep(registry.register(Arc::new(Verbose)));
    let store = MemorySpillStore::new();
    let mounted = root
        .provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
        .expect("cắm được");

    let pipeline = ToolPipeline::new(&root, registry.clone()).with_spill_threshold(100);
    let outcome = pipeline.execute("c1", "files__read", json!({})).await;

    assert!(!outcome.is_error);
    assert!(
        outcome.content.chars().count() < 500,
        "phần gửi cho mô hình vẫn dài"
    );
    let handle: SpillRef =
        serde_json::from_value(outcome.meta["spill"].clone()).expect("có vé lấy lại");
    assert_eq!(handle.chars, 5_000);
    assert_eq!(store.read(&handle).map(|s| s.len()), Some(5_000));
    drop(mounted);

    // Không có kho nào cắm vào thì thà gửi nguyên văn còn hơn mất phần đuôi.
    let bare = Context::root();
    let registry = ToolRegistry::new(&root);
    bare.keep(registry.register(Arc::new(Verbose)));
    let outcome = ToolPipeline::new(&bare, registry.clone())
        .with_spill_threshold(100)
        .execute("c1", "files__read", json!({}))
        .await;
    assert_eq!(outcome.content.chars().count(), 5_000);
}

// --- 8. từ vựng và tool mẫu -----------------------------------------------------------

/// Port của `test_dots_become_double_underscores_and_back`.
#[test]
fn dau_cham_thanh_gach_duoi_doi_va_nguoc_lai() {
    let name = ToolName::new("rag.graph.neighborhood");
    assert_eq!(name.wire(), "rag__graph__neighborhood");
    assert_eq!(ToolName::from_wire(&name.wire()), name);
    assert!(!name.wire().contains('.'));
    assert!(name.round_trips());
    // Một cái tên chứa sẵn `__` phá tính khả nghịch, nên sổ đăng ký từ chối nó.
    assert!(!ToolName::new("a__b").round_trips());
}

/// Một tool có tên không mã hoá khả nghịch được thì **không tồn tại**, chứ không tồn tại
/// dưới một cái tên mà chính sách không kiểm được.
#[tokio::test]
async fn ten_khong_ma_hoa_kha_nghich_thi_khong_duoc_dang_ky() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    root.keep(registry.register(Arc::new(Fake::new(
        "documents__delete",
        ToolMeta::mutating(),
    ))));
    assert!(registry.schemas(None).is_empty());

    let outcome = ToolPipeline::new(&root, registry.clone())
        .execute("c1", "documents__delete", json!({}))
        .await;
    assert!(outcome.is_error);
    assert_eq!(outcome.meta["refusal"], json!("unknown"));
}

/// `todo_write` là tool mẫu; bài này chỉ chứng minh nó đi qua đường ống được và trạng
/// thái của nó thuộc về phiên.
#[tokio::test]
async fn todo_write_ghi_de_ca_danh_sach_moi_lan() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    let todo = Arc::new(TodoWrite::new());
    root.keep(registry.register(todo.clone()));
    let pipeline = ToolPipeline::new(&root, registry.clone());

    let outcome = pipeline
        .execute(
            "c1",
            "todo_write",
            json!({ "todos": [
                { "content": "đọc mã", "status": "in_progress" },
                { "content": "viết test", "status": "pending" }
            ] }),
        )
        .await;
    assert!(!outcome.is_error, "{}", outcome.content);
    assert!(outcome.content.contains("- [~] đọc mã"));
    assert_eq!(todo.snapshot().len(), 2);

    // Ghi đè, không ghi thêm.
    let outcome = pipeline
        .execute(
            "c2",
            "todo_write",
            json!({ "todos": [{ "content": "xong", "status": "completed" }] }),
        )
        .await;
    assert!(!outcome.is_error);
    assert_eq!(todo.snapshot().len(), 1);

    // Sai luật của chính tool thì trả về văn bản, không phải một lượt kết thúc trong im lặng.
    let outcome = pipeline
        .execute(
            "c3",
            "todo_write",
            json!({ "todos": [
                { "content": "a", "status": "in_progress" },
                { "content": "b", "status": "in_progress" }
            ] }),
        )
        .await;
    assert!(outcome.is_error);
    assert!(outcome.content.contains("chỉ được một"));

    // Tham số ghim của mô hình không thấy `todo_write` — schema chỉ có `todos`.
    let schema = &registry.schemas(None)[0];
    assert_eq!(schema.name.wire(), "todo_write");
    assert!(
        schema.parameters["properties"]
            .as_object()
            .expect("object")
            .contains_key("todos")
    );
    assert!(schema.parameters.get("$schema").is_none());
}

/// Đăng ký có phạm vi che đăng ký toàn cục — đó là cách một agent thay một tool bằng bản
/// bị giam của riêng nó mà không đụng tới agent khác.
#[tokio::test]
async fn dang_ky_co_pham_vi_che_dang_ky_toan_cuc() {
    let (root, agent, registry, scope, tools) = bench();
    let sandboxed = Arc::new(Fake::new("documents.list", ToolMeta::read_only()));
    let shadow = registry.register_in(scope, sandboxed.clone());

    ToolPipeline::new(&agent, registry.clone())
        .execute("a", "documents__list", json!({}))
        .await;
    assert_eq!(sandboxed.ran.load(Ordering::SeqCst), 1);
    assert_eq!(tools[1].ran.load(Ordering::SeqCst), 0);

    // Một agent khác vẫn thấy bản toàn cục.
    let other = root.scoped("agent-b");
    ToolPipeline::new(&other, registry.clone())
        .execute("b", "documents__list", json!({}))
        .await;
    assert_eq!(tools[1].ran.load(Ordering::SeqCst), 1);
    assert_eq!(sandboxed.ran.load(Ordering::SeqCst), 1);

    // Danh sách quảng cáo không nhân đôi cái tên bị che.
    assert_eq!(
        names(&registry.schemas(Some(scope)))
            .iter()
            .filter(|n| *n == "documents.list")
            .count(),
        1
    );
    drop(shadow);
}

/// Nhiều hạn chế trên cùng một phạm vi thì **giao nhau**, và `deny` thắng `allow`.
#[tokio::test]
async fn nhieu_han_che_tren_cung_pham_vi_thi_giao_nhau() {
    let (_root, _agent, registry, scope, _tools) = bench();

    let a = registry.restrict(
        scope,
        ToolRestriction::allow_only(["workspaces.list", "documents.list", "documents.delete"]),
    );
    assert_eq!(registry.schemas(Some(scope)).len(), 3);

    let b = registry.restrict(
        scope,
        ToolRestriction::allow_only(["documents.list", "documents.delete"]),
    );
    assert_eq!(
        names(&registry.schemas(Some(scope))),
        vec!["documents.delete", "documents.list"]
    );

    let c = registry.restrict(scope, ToolRestriction::deny_only(["documents.delete"]));
    assert_eq!(
        names(&registry.schemas(Some(scope))),
        vec!["documents.list"]
    );

    // Gỡ một hạn chế chỉ gỡ đúng nó.
    drop(c);
    assert_eq!(registry.schemas(Some(scope)).len(), 2);
    drop(b);
    assert_eq!(registry.schemas(Some(scope)).len(), 3);
    drop(a);
    assert_eq!(registry.schemas(Some(scope)).len(), 4);
}

/// Không có [`pai_tools::Elicitor`] nào cắm vào thì hỏi giá trị trả `None` — fail-closed,
/// giống hệt phê duyệt.
#[tokio::test]
async fn khong_co_elicitor_thi_khong_hoi_duoc() {
    let call = Invocation::new(ToolName::new("files.read"), "c1", Map::new());
    assert_eq!(
        call.elicit("thư mục nào?", &json!({ "type": "string" }))
            .await,
        None
    );
}
