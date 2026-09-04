//! Loop invariants.
//! Each test locks one sentence of the module docs, and none touch the network: the model is
//! a fake adapter emitting scripted chunks, so its reply is a fixture, not an environment.

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use pai_agent::{Driver, PreStep, PreStepRequest, Silent, StepDecision, SystemPrompt};
use pai_core::{Context, Middleware, Next};
use pai_llm::{
    BlockKind, Capabilities, CapabilitySource, ChatRequest, FinishReason, LlmAdapter, LlmError,
    StreamChunk,
};
use pai_session::{Message, NewSession, Role, SessionService, SqliteSessionStore, TurnEndReason};
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolOutcome, ToolPipeline, ToolRegistry, ToolSchema,
};
use parking_lot::Mutex;
use serde_json::json;
use tokio_util::sync::CancellationToken;

// --- the fake model -------------------------------------------------------------------

/// Emits a scripted run, one script per call.
struct Script {
    turns: Mutex<Vec<Vec<StreamChunk>>>,
    seen: Mutex<Vec<ChatRequest>>,
    /// Stalls midway so a test can cancel while streaming.
    stall: bool,
}

impl Script {
    fn new(turns: Vec<Vec<StreamChunk>>) -> Arc<Script> {
        Arc::new(Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            stall: false,
        })
    }

    fn stalling(turns: Vec<Vec<StreamChunk>>) -> Arc<Script> {
        Arc::new(Script {
            turns: Mutex::new(turns),
            seen: Mutex::new(Vec::new()),
            stall: true,
        })
    }
}

#[async_trait]
impl LlmAdapter for Script {
    fn id(&self) -> &str {
        "kich-ban"
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        self.seen.lock().push(req);
        let mut turns = self.turns.lock();
        let chunks = if turns.is_empty() {
            Vec::new()
        } else {
            turns.remove(0)
        };
        let head = stream::iter(chunks.into_iter().map(Ok));
        if self.stall {
            // After the scripted part, sit still: cancellation ends the stream, as when the user hits Stop.
            Box::pin(head.chain(stream::once(async {
                std::future::pending::<()>().await;
                unreachable!()
            })))
        } else {
            Box::pin(head)
        }
    }

    async fn capabilities(&self, _model: &str) -> Result<Capabilities, LlmError> {
        Ok(Capabilities {
            chat: true,
            embedding: false,
            vision: false,
            tools: true,
            thinking: false,
            context_window: None,
            source: CapabilitySource::Reported,
        })
    }

    async fn health(&self) -> bool {
        true
    }
}

fn text(text: &str) -> Vec<StreamChunk> {
    vec![
        StreamChunk::BlockStart {
            index: 0,
            kind: BlockKind::Text,
        },
        StreamChunk::TextDelta {
            index: 0,
            text: text.to_string(),
        },
        StreamChunk::BlockEnd { index: 0 },
        StreamChunk::Finish {
            reason: FinishReason::Stop,
        },
    ]
}

fn calls_tool(name: &str, arguments: &str) -> Vec<StreamChunk> {
    vec![
        StreamChunk::BlockStart {
            index: 0,
            kind: BlockKind::ToolUse,
        },
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("goi-1".into()),
            name: Some(name.into()),
            arguments: arguments.to_string(),
        },
        StreamChunk::BlockEnd { index: 0 },
        StreamChunk::Finish {
            reason: FinishReason::ToolCalls,
        },
    ]
}

// --- the fake tool ----------------------------------------------------------------------

struct Echo;

#[async_trait]
impl Tool for Echo {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("echo", "Dội lại tham số.", json!({ "type": "object" }))
    }
    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }
    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::ok(format!("đã dội: {:?}", call.arguments)))
    }
}

// --- the harness ------------------------------------------------------------------------

struct Bench {
    ctx: Context,
    driver: Driver,
    service: SessionService,
}

fn bench(script: Arc<Script>, max_steps: u64) -> Bench {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    registry.register(Arc::new(Echo)).leak();
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry));
    let prompt = SystemPrompt::new();
    prompt
        .contribute(0, || Some("Bạn là trợ lý.".into()))
        .leak();

    let driver =
        Driver::new(ctx.clone(), script, pipeline, prompt, "mo-hinh").with_max_steps(max_steps);
    let store = Arc::new(SqliteSessionStore::open_in_memory().expect("mở kho"));
    Bench {
        ctx,
        driver,
        service: SessionService::new(store),
    }
}

async fn session(bench: &Bench) -> pai_session::Session {
    bench
        .service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên")
}

// --- the tests ----------------------------------------------------------------------------

#[tokio::test]
async fn mot_luot_mot_buoc() {
    let script = Script::new(vec![text("xin chào")]);
    let bench = bench(script.clone(), 12);
    let mut session = session(&bench).await;

    let reason = bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("chào")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt chạy được");

    assert!(matches!(reason, TurnEndReason::Completed));
    let history = session.derive_messages();
    assert_eq!(
        history.len(),
        2,
        "một message người dùng, một message trợ lý"
    );
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[1].role, Role::Assistant);
}

#[tokio::test]
async fn goi_tool_xong_thi_di_tiep_mot_buoc_nua() {
    let script = Script::new(vec![calls_tool("echo", "{\"a\":1}"), text("xong rồi")]);
    let bench = bench(script.clone(), 12);
    let mut session = session(&bench).await;

    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("chạy đi")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt chạy được");

    let history = session.derive_messages();
    let roles: Vec<Role> = history.iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant],
        "kết quả tool phải nằm giữa hai lần gọi mô hình"
    );
    assert_eq!(script.seen.lock().len(), 2, "gọi mô hình đúng hai lần");
}

#[tokio::test]
async fn lich_su_gui_cho_mo_hinh_dung_bang_phep_chieu_tu_so() {
    let script = Script::new(vec![calls_tool("echo", "{}"), text("hết")]);
    let bench = bench(script.clone(), 12);
    let mut session = session(&bench).await;

    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("hỏi")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt chạy được");

    // The second call must see exactly the journal's projection plus one system prompt; a second source of truth would drift.
    let seen = script.seen.lock();
    let second = seen.last().expect("có lần gọi thứ hai");
    let projected = session.derive_messages();
    assert_eq!(
        second.messages.len(),
        projected.len() - 1 + 1,
        "lịch sử gửi đi phải là phép chiếu từ sổ tại thời điểm gọi, cộng prompt hệ thống"
    );
    assert!(matches!(
        second.messages[0],
        pai_llm::Message::System { .. }
    ));
}

#[tokio::test]
async fn vong_cuoi_khong_duoc_trao_tool() {
    // A one-step ceiling: the first step is already the last round.
    let script = Script::new(vec![text("thôi vậy")]);
    let bench = bench(script.clone(), 1);
    let mut session = session(&bench).await;

    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("làm gì đi")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt chạy được");

    let seen = script.seen.lock();
    assert!(
        seen[0].tools.is_empty(),
        "vòng cuối mà vẫn trao tool thì lượt sẽ kết thúc bằng một lời gọi không ai trả lời"
    );
}

#[tokio::test]
async fn pre_step_tu_choi_van_ghi_mot_luot_khong_tieu_buoc_nao() {
    struct Reject;
    impl Middleware<PreStep> for Reject {
        fn call<'a>(
            &'a self,
            _req: &'a mut PreStepRequest,
            _next: Next<'a, PreStep>,
        ) -> futures::future::BoxFuture<'a, StepDecision> {
            Box::pin(async {
                StepDecision::Reject {
                    reason: "đang bận".into(),
                }
            })
        }
    }

    let script = Script::new(vec![text("không bao giờ tới đây")]);
    let bench = bench(script.clone(), 12);
    bench.ctx.on_waterfall::<PreStep>(Arc::new(Reject)).leak();
    let mut session = session(&bench).await;

    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("chào")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt chạy được");

    assert!(script.seen.lock().is_empty(), "không được gọi mô hình");
    assert!(
        session.derive_messages().is_empty(),
        "không có message nào vào lịch sử"
    );

    // The turn still has to be journalled: the record must remember the attempt.
    let types: Vec<String> = session
        .log()
        .events()
        .iter()
        .map(|e| e.event.type_name().to_string())
        .collect();
    assert!(types.contains(&"turn/start".to_string()));
    assert!(types.contains(&"turn/end".to_string()));
    assert!(
        !types.contains(&"step/start".to_string()),
        "không tiêu bước nào"
    );
}

#[tokio::test]
async fn huy_giua_stream_giu_lai_phan_tra_loi_do() {
    let partial = vec![
        StreamChunk::BlockStart {
            index: 0,
            kind: BlockKind::Text,
        },
        StreamChunk::TextDelta {
            index: 0,
            text: "đang viết dở".into(),
        },
    ];
    let script = Script::stalling(vec![partial]);
    let bench = bench(script, 12);
    let mut session = session(&bench).await;

    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        token.cancel();
    });

    let reason = bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("viết dài vào")],
            cancel,
            &Silent,
        )
        .await
        .expect("lượt kết thúc được");

    assert!(matches!(reason, TurnEndReason::Interrupted));
    let history = session.derive_messages();
    let assistant = history.last().expect("có message trợ lý");
    assert_eq!(assistant.role, Role::Assistant);
    // The whole point: half an answer must not vanish because the user pressed Stop.
    assert!(
        format!("{assistant:?}").contains("đang viết dở"),
        "mất phần trả lời dở: {assistant:?}"
    );
}

/// A sink that records `meta`, to prove it is not dropped on the way.
#[derive(Default)]
struct MetaSink {
    seen: Mutex<Vec<serde_json::Map<String, serde_json::Value>>>,
}

impl pai_agent::TurnSink for MetaSink {
    fn tool_end(
        &self,
        _call_id: &str,
        _name: &str,
        _is_error: bool,
        _preview: &str,
        meta: &serde_json::Map<String, serde_json::Value>,
    ) {
        self.seen.lock().push(meta.clone());
    }
}

struct Rich;

#[async_trait]
impl Tool for Rich {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "rich",
            "Trả về meta cho giao diện.",
            json!({ "type": "object" }),
        )
    }
    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }
    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::ok("xong").with_meta("diffs", json!([{ "path": "a.rs" }])))
    }
}

#[tokio::test]
async fn meta_cua_tool_di_toi_duoc_giao_dien() {
    let script = Script::new(vec![calls_tool("rich", "{}"), text("hết")]);
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    registry.register(Arc::new(Rich)).leak();
    let prompt = SystemPrompt::new();
    let driver = Driver::new(
        ctx.clone(),
        script,
        Arc::new(ToolPipeline::new(&ctx, registry)),
        prompt,
        "mo-hinh",
    );
    let store = Arc::new(SqliteSessionStore::open_in_memory().expect("mở kho"));
    let service = SessionService::new(store);
    let mut session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");

    let sink = MetaSink::default();
    driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("chạy")],
            CancellationToken::new(),
            &sink,
        )
        .await
        .expect("lượt chạy được");

    // This is what draws the diff block; losing it leaves a UI that still works and says nothing.
    let seen = sink.seen.lock();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].contains_key("diffs"),
        "meta bị rơi trên đường tới giao diện: {:?}",
        seen[0]
    );
}

/// Records every `notice` the driver emits during a turn.
struct NoticeSink {
    said: Mutex<Vec<String>>,
}

impl pai_agent::TurnSink for NoticeSink {
    fn notice(&self, message: &str) {
        self.said.lock().push(message.to_string());
    }
}

/// Middleware that always summarises the first two messages, so the test does not depend on the pressure threshold.
struct AlwaysCompact;

impl Middleware<PreStep> for AlwaysCompact {
    fn call<'a>(
        &'a self,
        req: &'a mut PreStepRequest,
        next: Next<'a, PreStep>,
    ) -> futures::future::BoxFuture<'a, StepDecision> {
        Box::pin(async move {
            match next.run(req).await {
                StepDecision::Enter {
                    messages,
                    replace: None,
                } => StepDecision::Enter {
                    messages,
                    replace: Some(pai_agent::Replacement {
                        start: 0,
                        end: 2,
                        summary: Message::user("<tóm tắt>"),
                    }),
                },
                decided => decided,
            }
        })
    }
}

/// Compaction has to say so; silent loss just looks like the model getting worse.
#[tokio::test]
async fn nen_ngu_canh_thi_phai_noi_ra() {
    let script = Script::new(vec![text("xong")]);
    let bench = bench(script, 12);
    bench
        .ctx
        .on_waterfall_first::<PreStep>(Arc::new(AlwaysCompact))
        .leak();

    let mut session = session(&bench).await;
    for i in 0..3 {
        session
            .append_surface(pai_session::SessionEvent::UserMessage(Message::user(
                format!("câu {i}"),
            )))
            .await
            .expect("ghi vào sổ");
    }

    let sink = NoticeSink {
        said: Mutex::new(Vec::new()),
    };
    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("tiếp đi")],
            CancellationToken::new(),
            &sink,
        )
        .await
        .expect("lượt chạy được");

    let said = sink.said.lock().clone();
    assert!(
        said.iter().any(|line| line.contains("rút gọn")),
        "người dùng phải được báo là ngữ cảnh vừa bị nén, nhưng driver chỉ nói: {said:?}"
    );
    assert!(
        said.iter().any(|line| line.contains('2')),
        "câu báo phải nói **bao nhiêu** tin nhắn đã mất nguyên văn: {said:?}"
    );
}

/// Records the token counts the driver reports.
struct UsageSink {
    seen: Mutex<Vec<(u64, u64)>>,
}

impl pai_agent::TurnSink for UsageSink {
    fn usage(&self, input_tokens: u64, output_tokens: u64) {
        self.seen.lock().push((input_tokens, output_tokens));
    }
}

/// Server-reported token counts must reach the journal; they were once parsed and then thrown away.
#[tokio::test]
async fn so_token_may_chu_bao_phai_di_tiep() {
    let mut turn = text("xong");
    turn.insert(
        turn.len() - 1,
        StreamChunk::Usage {
            usage: pai_llm::TokenUsage {
                input_tokens: 1234,
                output_tokens: 56,
                total_tokens: None,
            },
        },
    );

    let script = Script::new(vec![turn]);
    let bench = bench(script, 12);
    let mut session = session(&bench).await;

    let sink = UsageSink {
        seen: Mutex::new(Vec::new()),
    };
    bench
        .driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("chào")],
            CancellationToken::new(),
            &sink,
        )
        .await
        .expect("lượt chạy được");

    assert_eq!(
        sink.seen.lock().clone(),
        vec![(1234, 56)],
        "driver phải báo lại đúng con số máy chủ đã nói"
    );
}
