//! Switching providers mid-turn. The invariant: one turn talks to exactly one server and one model, even
//! if the user switches while it runs. A real two-step turn pauses after the first request, swaps the model,
//! and checks which name step two sends. The fake adapter blocks on a [`Semaphore`], never on time.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use pai_agent::{Driver, Silent, SystemPrompt};
use pai_core::Context;
use pai_llm::{
    BlockKind, Capabilities, CapabilitySource, ChatRequest, FinishReason, LlmAdapter, LlmError,
    StreamChunk,
};
use pai_session::{Message, NewSession, SessionService, SqliteSessionStore};
use pai_tools::{ToolPipeline, ToolRegistry};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

/// Fake model: records each request's model name and waits for a permit before emitting any chunk.
struct Cong {
    models: Mutex<Vec<String>>,
    kich_ban: Mutex<Vec<Vec<StreamChunk>>>,
    bao: UnboundedSender<()>,
    cong: Arc<Semaphore>,
}

#[async_trait]
impl LlmAdapter for Cong {
    fn id(&self) -> &str {
        "cong"
    }

    fn stream(&self, req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        self.models
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(req.model.clone());
        let mut kich_ban = self.kich_ban.lock().unwrap_or_else(|p| p.into_inner());
        let chunks = if kich_ban.is_empty() {
            Vec::new()
        } else {
            kich_ban.remove(0)
        };
        let bao = self.bao.clone();
        let cong = self.cong.clone();
        stream::once(async move {
            // Signal that the request is built (so the model name is fixed), then block; the test swaps in this window.
            let _ = bao.send(());
            if let Ok(permit) = cong.acquire().await {
                permit.forget();
            }
            stream::iter(chunks.into_iter().map(Ok))
        })
        .flatten()
        .boxed()
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
}

fn goi_tool() -> Vec<StreamChunk> {
    vec![
        StreamChunk::BlockStart {
            index: 0,
            kind: BlockKind::ToolUse,
        },
        StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("goi-1".into()),
            name: Some("khong-co-tool-nay".into()),
            arguments: "{}".into(),
        },
        StreamChunk::BlockEnd { index: 0 },
        StreamChunk::Finish {
            reason: FinishReason::ToolCalls,
        },
    ]
}

fn tra_loi() -> Vec<StreamChunk> {
    vec![
        StreamChunk::BlockStart {
            index: 0,
            kind: BlockKind::Text,
        },
        StreamChunk::TextDelta {
            index: 0,
            text: "xong".into(),
        },
        StreamChunk::BlockEnd { index: 0 },
        StreamChunk::Finish {
            reason: FinishReason::Stop,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doi_mo_hinh_giua_luot_khong_dong_toi_luot_dang_chay() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry));
    let prompt = SystemPrompt::new();

    let (bao, mut nhan) = unbounded_channel();
    let cong = Arc::new(Semaphore::new(0));
    // Two steps: the first calls a nonexistent tool (the pipeline refuses and the loop continues), the second answers and ends the turn.
    let adapter = Arc::new(Cong {
        models: Mutex::new(Vec::new()),
        kich_ban: Mutex::new(vec![goi_tool(), tra_loi()]),
        bao,
        cong: cong.clone(),
    });

    let driver = Arc::new(Driver::new(
        ctx,
        adapter.clone(),
        pipeline,
        prompt,
        "mo-hinh-cu",
    ));
    let service = SessionService::new(Arc::new(
        SqliteSessionStore::open_in_memory().expect("mở sổ"),
    ));
    let mut session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");

    let chay = tokio::spawn({
        let driver = driver.clone();
        async move {
            driver
                .run_turn(
                    &mut session,
                    1,
                    vec![Message::user("chào")],
                    CancellationToken::new(),
                    &Silent,
                )
                .await
        }
    });

    nhan.recv().await.expect("bước một đã gửi request");
    driver.set_llm(adapter.clone());
    driver.set_model("mo-hinh-moi");
    cong.add_permits(1);

    nhan.recv().await.expect("bước hai đã gửi request");
    cong.add_permits(1);

    chay.await.expect("lượt chạy xong").expect("lượt không lỗi");

    let da_gui = adapter
        .models
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    assert_eq!(
        da_gui,
        vec!["mo-hinh-cu".to_string(), "mo-hinh-cu".to_string()],
        "bước hai của cùng một lượt phải giữ nguyên mô hình đã chốt lúc mở lượt"
    );
    // The switch is not lost: it waits for the next turn.
    assert_eq!(driver.model(), "mo-hinh-moi");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn luot_sau_moi_an_cai_vua_doi() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry));
    let prompt = SystemPrompt::new();

    let (bao, mut nhan) = unbounded_channel();
    let cong = Arc::new(Semaphore::new(8));
    let adapter = Arc::new(Cong {
        models: Mutex::new(Vec::new()),
        kich_ban: Mutex::new(vec![tra_loi(), tra_loi()]),
        bao,
        cong,
    });
    let driver = Driver::new(ctx, adapter.clone(), pipeline, prompt, "mo-hinh-cu");
    let service = SessionService::new(Arc::new(
        SqliteSessionStore::open_in_memory().expect("mở sổ"),
    ));
    let mut session = service
        .create(NewSession::in_dir("/tmp"))
        .await
        .expect("tạo phiên");

    driver
        .run_turn(
            &mut session,
            1,
            vec![Message::user("một")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt một");
    driver.set_model("mo-hinh-moi");
    driver
        .run_turn(
            &mut session,
            2,
            vec![Message::user("hai")],
            CancellationToken::new(),
            &Silent,
        )
        .await
        .expect("lượt hai");

    nhan.close();
    let da_gui = adapter
        .models
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    assert_eq!(
        da_gui,
        vec!["mo-hinh-cu".to_string(), "mo-hinh-moi".to_string()]
    );
}
