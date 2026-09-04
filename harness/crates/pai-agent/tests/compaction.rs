//! Context compaction.
//! The boundary test matters most: cutting between a tool call and its result gets the
//! request rejected outright, and the symptom ("400 from the server") points nowhere near.

use std::sync::Arc;

use pai_agent::{CompactionPlugin, PreStep, PreStepRequest, StepDecision};
use pai_core::{Context, Plugin};
use pai_session::{ContentBlock, Message, Role};

/// A small window so pressure arrives early; the plugin floors it at 2048.
const WINDOW: usize = 2048;

fn long_user(n: usize) -> Message {
    Message::user("x".repeat(n))
}

fn tool_pair(id: &str) -> Vec<Message> {
    vec![
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall {
                id: id.into(),
                name: "read".into(),
                arguments: "{}".into(),
            }],
            source: None,
        },
        Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                call_id: id.into(),
                content: "y".repeat(2000),
                is_error: false,
            }],
            source: None,
        },
    ]
}

async fn decide(ctx: &Context, history: Vec<Message>) -> StepDecision {
    let mut req = PreStepRequest {
        turn: 1,
        step: 1,
        messages: vec![Message::user("tiếp đi")],
        history,
    };
    ctx.waterfall::<PreStep, _>(&mut req, |req| {
        let messages = req.messages.clone();
        Box::pin(async move { StepDecision::enter(messages) })
    })
    .await
}

async fn ctx_with_compaction() -> Context {
    let ctx = Context::root();
    let scope = ctx.plugin("compaction");
    CompactionPlugin::new(WINDOW)
        .apply(&scope)
        .await
        .expect("cắm được");
    std::mem::forget(scope);
    ctx
}

#[tokio::test]
async fn duoi_nguong_thi_khong_dung_toi() {
    let ctx = ctx_with_compaction().await;
    let decision = decide(&ctx, vec![long_user(100)]).await;
    assert!(matches!(
        decision,
        StepDecision::Enter { replace: None, .. }
    ));
}

#[tokio::test]
async fn vuot_nguong_thi_che_phan_dau_va_giu_phan_duoi() {
    let ctx = ctx_with_compaction().await;
    // Ten nodes at about 1000 tokens each: far past 80% of 2048.
    let history: Vec<Message> = (0..10).map(|_| long_user(4000)).collect();
    let total = history.len();

    let StepDecision::Enter {
        replace: Some(replace),
        ..
    } = decide(&ctx, history).await
    else {
        panic!("phải yêu cầu che");
    };
    assert_eq!(replace.start, 0);
    assert!(
        replace.end > 0 && replace.end < total,
        "che {} trên {total} node",
        replace.end
    );
    // The summary is a real, readable message that says it is a summary.
    assert!(format!("{:?}", replace.summary).contains("ngu-canh-da-nen"));
}

#[tokio::test]
async fn khong_cat_giua_mot_loi_goi_tool_va_ket_qua_cua_no() {
    let ctx = ctx_with_compaction().await;
    let mut history: Vec<Message> = Vec::new();
    for i in 0..6 {
        history.push(long_user(1200));
        history.extend(tool_pair(&format!("goi-{i}")));
    }

    let StepDecision::Enter {
        replace: Some(replace),
        ..
    } = decide(&ctx, history.clone()).await
    else {
        panic!("phải yêu cầu che");
    };

    // The first surviving node must not be an orphan result, nor the last masked one a pending call.
    if let Some(first_kept) = history.get(replace.end) {
        assert_ne!(
            first_kept.role,
            Role::Tool,
            "để lại một kết quả tool không có lời gọi"
        );
    }
    if replace.end > 0 {
        let last_dropped = &history[replace.end - 1];
        let opens = last_dropped
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolCall { .. }));
        assert!(!opens, "che mất kết quả nhưng giữ lại lời gọi");
    }
}

#[tokio::test]
async fn khong_dam_len_quyet_dinh_cua_tang_duoi() {
    let ctx = ctx_with_compaction().await;
    let history: Vec<Message> = (0..10).map(|_| long_user(4000)).collect();

    // Another layer rejects the step, and compaction must not turn that refusal into an entry.
    struct Reject;
    impl pai_core::Middleware<PreStep> for Reject {
        fn call<'a>(
            &'a self,
            _req: &'a mut PreStepRequest,
            _next: pai_core::Next<'a, PreStep>,
        ) -> futures::future::BoxFuture<'a, StepDecision> {
            Box::pin(async {
                StepDecision::Reject {
                    reason: "đang bận".into(),
                }
            })
        }
    }
    ctx.on_waterfall::<PreStep>(Arc::new(Reject)).leak();

    assert!(matches!(
        decide(&ctx, history).await,
        StepDecision::Reject { .. }
    ));
}
