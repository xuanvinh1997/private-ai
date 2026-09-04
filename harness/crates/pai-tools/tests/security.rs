//! The crate's security specification.
//! The first four groups are invariants ported from the Python suite; the last three are
//! Rust-only: monotonic guards, fail-closed approval, and long output that is never lost.

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

// --- a fake tool that can prove whether it ran -------------------------------------------

struct Fake {
    name: ToolName,
    meta: ToolMeta,
    parameters: Value,
    /// How many times the body actually ran; the proof a refusal happened before the tool was touched.
    ran: Arc<AtomicUsize>,
    /// The arguments the body saw, after pinning and after every middleware.
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

/// Four tools, enough to draw the read-only boundary: two that look, two that touch.
fn library() -> Vec<Arc<Fake>> {
    vec![
        Arc::new(Fake::new("workspaces.list", ToolMeta::read_only())),
        Arc::new(Fake::new("documents.list", ToolMeta::read_only())),
        Arc::new(Fake::new("documents.ingest_text", ToolMeta::mutating())),
        Arc::new(Fake::new("documents.delete", ToolMeta::mutating())),
    ]
}

/// Build a scoped agent with the tool library already mounted globally.
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

// --- 1. the allow set is exactly the non-mutating tools -----------------------------------

/// Locks that a read-only agent is advertised exactly the tools with `mutating == false`, and not one more.
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

    // The two sets are disjoint, which is the invariant `READ_ONLY_TOOLS` exists to hold.
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
    // The host, with no scope, still sees everything: a direct UI call is not restricted.
    assert_eq!(registry.schemas(None).len(), 4);
    drop(guard);
}

// --- 2. refused even when called by the encoded name --------------------------------------

/// Locks that the advertised list is only a hint: a guessed wire name is stopped by the second filter.
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

    // The wire name, exactly what the model can type.
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
    // A refusal is text, not a `Result` thrown upward.
    assert!(!refused.content.is_empty());
    // And nothing was written.
    assert_eq!(ingest.ran.load(Ordering::SeqCst), 0);

    let refused = pipeline
        .execute("c2", "documents__delete", json!({ "confirmed": true }))
        .await;
    assert!(refused.is_error);
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);

    // On the same path an allowed tool still runs: the filter filters, it does not block everything.
    let ok = pipeline.execute("c3", "workspaces__list", json!({})).await;
    assert!(!ok.is_error);
    assert_eq!(tools[0].ran.load(Ordering::SeqCst), 1);
    drop(guard);
}

// --- 3. refusal happens before the tool body is touched -----------------------------------

/// Locks that the body never runs: restriction, `tools/pre-execute` and guards all stop before the tool is touched.
#[tokio::test]
async fn tu_choi_xay_ra_truoc_khi_cham_vao_than_tool() {
    // (a) a scope restriction
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

    // (b) `tools/pre-execute` says no
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

    // (c) a guard
    let gate = registry.add_guard(Some(scope), Arc::new(DenyAll));
    let denied = pipeline.execute("c", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert_eq!(denied.content, "canh gác nói không");
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(gate);

    // With every policy removed the tool runs, so the three cases above measure policy, not a broken tool.
    assert!(
        !pipeline
            .execute("d", "documents__delete", json!({}))
            .await
            .is_error
    );
    assert_eq!(delete.ran.load(Ordering::SeqCst), 1);
}

// --- 4. a pinned parameter leaves the schema and is overridden at call time ---------------

/// Locks that a parameter the model cannot see is one it cannot get wrong: pinned values override, never default.
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

    // Before pinning the model sees all three parameters.
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

    // A "harmless" middleware tries to reset workspace_id; the pin must still win, or the hook is a bypass.
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

// --- 5. monotonic guards cannot be reversed -----------------------------------------------

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

/// The most permissive guard the trait allows: it can still only abstain.
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

/// Locks that guard registration order cannot change the answer, since [`ToolGuard`] has no allow branch.
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

// --- 6. fail-closed approval ---------------------------------------------------------------

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
        // A dialog hidden behind another window. It never answers.
        std::future::pending::<()>().await;
        true
    }
}

/// Locks that no approver and a timeout both mean refusal; no branch turns "cannot ask" into "go ahead".
#[tokio::test]
async fn khong_co_approver_hoac_het_gio_deu_la_tu_choi() {
    let (root, agent, registry, _scope, tools) = bench();
    let delete = tools[3].clone();
    let ask = agent.on_waterfall::<PreExecute>(Arc::new(AlwaysAsk));
    let pipeline = ToolPipeline::new(&agent, registry.clone())
        .with_approval_timeout(std::time::Duration::from_millis(50));

    // (a) nobody to ask
    let denied = pipeline.execute("a", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert!(denied.content.contains("không cho phép"));
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);

    // (b) someone to ask who never answers
    let hung: Arc<dyn Approver> = Arc::new(NeverAnswers);
    let mounted = root.provide::<Approval>(hung).expect("cắm được");
    let denied = pipeline.execute("b", "documents__delete", json!({})).await;
    assert!(denied.is_error);
    assert_eq!(delete.ran.load(Ordering::SeqCst), 0);
    drop(mounted);

    // (c) the user says no
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

    // (d) the user says yes, and only then does the tool run
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

/// Locks that approval cannot open what a guard closed, because guards run afterwards.
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

// --- 7. the trust boundary and the spill store --------------------------------------------

/// Locks that the notice lives in the tool description and is inserted by the registry, not by each tool author.
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
    // Not pasted onto tools returning no outside content: a warning everywhere is a warning nowhere.
    assert!(!time.description.contains(UNTRUSTED_NOTICE));

    // And host metadata does not travel with it: the schema has exactly three fields.
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

/// Locks that long output is kept whole: the threshold decides how much the model reads, not what survives.
#[tokio::test]
async fn output_dai_duoc_cat_vao_kho_chu_khong_bi_cat_cut() {
    let root = Context::root();
    let registry = ToolRegistry::new(&root);
    root.keep(registry.register(Arc::new(Verbose)));
    let store = MemorySpillStore::new();
    let mounted = root
        .provide::<Spill>(store.clone() as Arc<dyn SpillStore>)
        .expect("cắm được");

    let pipeline = ToolPipeline::new(&root, registry.clone()).with_token_budget(25);
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

    // With no store mounted, sending the whole text beats losing the tail.
    let bare = Context::root();
    let registry = ToolRegistry::new(&root);
    bare.keep(registry.register(Arc::new(Verbose)));
    let outcome = ToolPipeline::new(&bare, registry.clone())
        .with_token_budget(25)
        .execute("c1", "files__read", json!({}))
        .await;
    assert_eq!(outcome.content.chars().count(), 5_000);
}

// --- 8. vocabulary and the reference tool -------------------------------------------------

/// Dots become double underscores and back again.
#[test]
fn dau_cham_thanh_gach_duoi_doi_va_nguoc_lai() {
    let name = ToolName::new("rag.graph.neighborhood");
    assert_eq!(name.wire(), "rag__graph__neighborhood");
    assert_eq!(ToolName::from_wire(&name.wire()), name);
    assert!(!name.wire().contains('.'));
    assert!(name.round_trips());
    // A name already containing `__` is irreversible, so the registry refuses it.
    assert!(!ToolName::new("a__b").round_trips());
}

/// A tool whose name cannot round-trip does not exist, rather than existing under an uncheckable name.
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

/// `todo_write` is the reference tool; this only shows it passes the pipeline and its state is session-scoped.
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

    // Replace, not append.
    let outcome = pipeline
        .execute(
            "c2",
            "todo_write",
            json!({ "todos": [{ "content": "xong", "status": "completed" }] }),
        )
        .await;
    assert!(!outcome.is_error);
    assert_eq!(todo.snapshot().len(), 1);

    // Breaking the tool's own rule returns text, not a silently ended turn.
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

    // `todo_write` has no pinned parameters: its schema holds only `todos`.
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

/// A scoped registration shadows the global one, which is how an agent swaps in its own sandboxed version.
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

    // Another agent still sees the global one.
    let other = root.scoped("agent-b");
    ToolPipeline::new(&other, registry.clone())
        .execute("b", "documents__list", json!({}))
        .await;
    assert_eq!(tools[1].ran.load(Ordering::SeqCst), 1);
    assert_eq!(sandboxed.ran.load(Ordering::SeqCst), 1);

    // The advertised list does not duplicate the shadowed name.
    assert_eq!(
        names(&registry.schemas(Some(scope)))
            .iter()
            .filter(|n| *n == "documents.list")
            .count(),
        1
    );
    drop(shadow);
}

/// Several restrictions on one scope intersect, and `deny` beats `allow`.
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

    // Dropping one restriction drops only that one.
    drop(c);
    assert_eq!(registry.schemas(Some(scope)).len(), 2);
    drop(b);
    assert_eq!(registry.schemas(Some(scope)).len(), 3);
    drop(a);
    assert_eq!(registry.schemas(Some(scope)).len(), 4);
}

/// With no [`pai_tools::Elicitor`] mounted, asking for a value returns `None` — fail-closed, as approval is.
#[tokio::test]
async fn khong_co_elicitor_thi_khong_hoi_duoc() {
    let call = Invocation::new(ToolName::new("files.read"), "c1", Map::new());
    assert_eq!(
        call.elicit("thư mục nào?", &json!({ "type": "string" }))
            .await,
        None
    );
}
