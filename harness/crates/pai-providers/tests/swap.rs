//! Đổi provider giữa một lượt đang chạy.
//!
//! Bất biến được khoá ở đây: **một lượt nói chuyện với đúng một máy chủ và đúng một mô
//! hình**, kể cả khi người dùng bấm đổi trong lúc lượt đang dở. Bài này chạy một lượt
//! thật hai bước, dừng đúng lúc mô hình vừa nhận request đầu tiên, đổi mô hình, rồi xem
//! bước thứ hai gửi đi tên nào.
//!
//! Adapter giả dừng lại bằng một [`Semaphore`] chứ không bằng `sleep`: một bài kiểm chứng
//! dựa trên thời gian là một bài kiểm chứng sẽ chớp tắt trên máy bận.

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

/// Mô hình giả: ghi lại tên mô hình của từng request, và đứng chờ một giấy phép trước khi
/// phát ra chunk nào.
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
            // Báo là request đã được dựng — tức là tên mô hình đã được chốt — rồi đứng
            // lại. Bài kiểm chứng đổi mô hình đúng trong khoảng này.
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
    // Hai bước: bước một gọi một tool không tồn tại (đường ống trả về một câu từ chối,
    // và vòng lặp đi tiếp), bước hai trả lời rồi đóng lượt.
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
    // Cú đổi không bị mất: nó chờ tới lượt sau.
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
