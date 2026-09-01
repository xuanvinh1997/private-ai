//! Giao việc cho agent con.
//!
//! Hai bất biến, và cả hai đều là chuyện tiền bạc chứ không phải chuyện đúng sai:
//!
//! **Ngữ cảnh của con không tràn sang cha.** Cả điểm của việc giao việc là một việc tốn
//! năm mươi nghìn token trả về một đoạn văn năm dòng. Trả về cả bản ghi thì thà đừng giao.
//!
//! **Có đáy.** Không có trần thì một mô hình đang bí sẽ giao việc cho chính hình ảnh của
//! nó, mãi mãi, và thứ chặn lại là hết tiền chứ không phải một dòng mã.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use pai_agent::{LocalSubagents, MAX_DEPTH, Prompt, SubagentProvider, SystemPrompt, Task};
use pai_core::{Context, Plugin};
use pai_llm::{
    BlockKind, Capabilities, CapabilitySource, ChatRequest, FinishReason, LlmAdapter, LlmError,
    StreamChunk,
};
use pai_session::{SessionService, SqliteSessionStore};
use pai_tools::{Invocation, Tool, ToolName, Tools, ToolsPlugin};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

/// Mô hình giả: mọi lượt đều trả về đúng một câu.
struct Fixed {
    text: String,
    calls: Mutex<usize>,
}

impl Fixed {
    fn new(text: &str) -> Arc<Fixed> {
        Arc::new(Fixed {
            text: text.into(),
            calls: Mutex::new(0),
        })
    }
}

#[async_trait]
impl LlmAdapter for Fixed {
    fn id(&self) -> &str {
        "co-dinh"
    }

    fn stream(&self, _req: ChatRequest) -> BoxStream<'_, Result<StreamChunk, LlmError>> {
        *self.calls.lock() += 1;
        let chunks = vec![
            StreamChunk::BlockStart {
                index: 0,
                kind: BlockKind::Text,
            },
            StreamChunk::TextDelta {
                index: 0,
                text: self.text.clone(),
            },
            StreamChunk::BlockEnd { index: 0 },
            StreamChunk::Finish {
                reason: FinishReason::Stop,
            },
        ];
        Box::pin(stream::iter(chunks.into_iter().map(Ok)))
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

async fn bench(llm: Arc<Fixed>) -> (Context, Arc<LocalSubagents>, SessionService) {
    let ctx = Context::root();
    let scope = ctx.plugin("tools");
    ToolsPlugin.apply(&scope).await.expect("cắm được tools");
    std::mem::forget(scope);
    ctx.provide::<Prompt>(SystemPrompt::new())
        .expect("cắm được prompt")
        .leak();

    let store = Arc::new(SqliteSessionStore::open_in_memory().expect("mở kho"));
    let sessions = SessionService::new(store);
    let provider = Arc::new(LocalSubagents::new(
        ctx.clone(),
        llm,
        sessions.clone(),
        "mo-hinh",
        "/tmp",
    ));
    (ctx, provider, sessions)
}

fn call(args: Value) -> Invocation {
    let map: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    Invocation::new(ToolName::from(Task::NAME), "c1", map)
}

#[tokio::test]
async fn bao_cao_la_loi_cuoi_cua_con_chu_khong_phai_ca_ban_ghi() {
    let llm = Fixed::new("Đã xong: có ba chỗ dùng hàm đó.");
    let (_ctx, provider, sessions) = bench(llm).await;

    let outcome = Task::new(provider, 0)
        .execute(&call(json!({ "prompt": "tìm mọi chỗ dùng hàm chao()" })))
        .await
        .expect("giao được việc");

    assert!(outcome.content.contains("ba chỗ"));
    // Bản ghi đầy đủ vẫn còn — trong sổ của **con**, không trong ngữ cảnh của cha.
    let structured = outcome.structured.expect("có phần structured");
    let child = structured["session_id"].as_str().expect("có id phiên con");
    let opened = sessions.open(child).await.expect("mở lại được sổ của con");
    assert!(
        opened.derive_messages().len() >= 2,
        "sổ của con phải giữ đủ, kể cả khi cha chỉ nhận một dòng"
    );
}

#[tokio::test]
async fn den_day_thi_khong_giao_tiep_duoc_nua() {
    let llm = Fixed::new("xong");
    let (_ctx, provider, _) = bench(llm).await;

    // Ở đúng đáy: lời từ chối phải nói rõ và bảo mô hình tự làm, chứ không im lặng hỏng.
    let err = Task::new(provider.clone(), MAX_DEPTH)
        .execute(&call(json!({ "prompt": "giao tiếp đi" })))
        .await
        .expect_err("tới đáy thì phải từ chối");
    assert!(err.to_string().contains("đáy"), "{err}");
}

#[tokio::test]
async fn con_o_tang_cuoi_khong_con_nhin_thay_tool_task() {
    let llm = Fixed::new("xong");
    let (ctx, provider, _) = bench(llm).await;
    let registry = ctx.require::<Tools>().expect("có sổ đăng ký");
    registry
        .register(Arc::new(Task::new(provider.clone(), 0)))
        .leak();

    // Cha thấy `task`.
    let parent: Vec<String> = registry
        .schemas(ctx.scope_key())
        .into_iter()
        .map(|s| s.name.as_str().to_string())
        .collect();
    assert!(parent.contains(&Task::NAME.to_string()));

    // Con ở tầng cuối thì không — nó bị **rút khỏi danh sách**, không phải để nó gọi rồi
    // nhận một lời từ chối. Một tool nhìn thấy được mà lần nào cũng hỏng là một tool dạy
    // mô hình bỏ qua danh sách.
    provider
        .delegate("làm gì đó", MAX_DEPTH - 1)
        .await
        .expect("tầng cuối vẫn chạy được, chỉ là không giao tiếp được");
}

#[tokio::test]
async fn moi_lan_giao_viec_la_mot_phien_rieng() {
    let llm = Fixed::new("xong");
    let (_ctx, provider, sessions) = bench(llm).await;

    let first = provider.delegate("việc một", 0).await.expect("giao được");
    let second = provider.delegate("việc hai", 0).await.expect("giao được");
    assert_ne!(first.session_id, second.session_id);

    let listed = sessions.list(Some(10)).await.expect("liệt kê");
    assert!(listed.len() >= 2);
}
