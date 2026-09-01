//! Phép chiếu sổ → lịch sử mô hình, và những bất biến mà DDL không diễn đạt được.

use pai_session::{
    AssistantMessage, ContentBlock, Message, Role, SessionError, SessionEvent,
    SessionEventEnvelope, SessionLog, StepStart, SurfaceOp, ToolResult, TurnEnd, TurnEndReason,
    TurnStart,
};

const T: i64 = 1_700_000_000_000;

fn user(text: &str) -> SessionEvent {
    SessionEvent::UserMessage(Message::user(text))
}

fn assistant(text: &str) -> SessionEvent {
    SessionEvent::AssistantMessage(AssistantMessage {
        turn: 0,
        step: 0,
        message: Message::assistant(text),
        usage: None,
        interrupted: None,
    })
}

/// Bước bị cụt vì hết token: message tồn tại chỉ để giữ `usage`, nội dung rỗng.
fn assistant_rong() -> SessionEvent {
    SessionEvent::AssistantMessage(AssistantMessage {
        turn: 0,
        step: 0,
        message: Message {
            role: Role::Assistant,
            content: Vec::new(),
            source: None,
        },
        usage: Some(pai_session::Usage {
            input_tokens: 12,
            output_tokens: 0,
            cached_input_tokens: None,
        }),
        interrupted: None,
    })
}

fn texts(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .map(|m| match m.content.first() {
            Some(ContentBlock::Text { text }) => text.clone(),
            _ => "<không phải chữ>".to_owned(),
        })
        .collect()
}

#[test]
fn seq_lien_mach_khong_ho() {
    let mut log = SessionLog::new();
    log.append(SessionEvent::TurnStart(TurnStart { turn: 0 }), T)
        .expect("turn/start");
    log.append(SessionEvent::StepStart(StepStart { turn: 0, step: 0 }), T)
        .expect("step/start");
    log.append_surface(user("chào"), T).expect("user/message");
    log.append_surface(assistant("chào lại"), T)
        .expect("assistant/message");
    log.append(
        SessionEvent::TurnEnd(TurnEnd {
            turn: 0,
            reason: TurnEndReason::Completed,
        }),
        T,
    )
    .expect("turn/end");

    for (index, envelope) in log.events().iter().enumerate() {
        assert_eq!(envelope.seq, index as u64, "seq phải bằng chỉ số");
    }
    assert_eq!(log.next_seq(), 5);
}

#[test]
fn phat_lai_mot_so_co_lo_hong_bi_tu_choi() {
    let mut good = SessionLog::new();
    good.append_surface(user("a"), T).expect("a");
    good.append_surface(user("b"), T).expect("b");

    let mut events = good.events().to_vec();
    events.remove(0);
    match SessionLog::replay(events) {
        Err(SessionError::SeqGap {
            expected: 0,
            found: 1,
        }) => {}
        other => panic!("phải là SeqGap, gặp {other:?}", other = other.err()),
    }
}

#[test]
fn append_thuan_giu_nguyen_thu_tu() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    log.append_surface(assistant("b"), T).expect("b");
    log.append_surface(user("c"), T).expect("c");

    assert_eq!(texts(&log.derive_messages()), ["a", "b", "c"]);
    assert_eq!(log.surface().generation(), 0, "append không làm đổi thế hệ");
}

#[test]
fn replace_mot_dai_giua_khong_xoa_gi_ca() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    log.append_surface(assistant("b"), T).expect("b");
    log.append_surface(user("c"), T).expect("c");
    log.append_surface(assistant("d"), T).expect("d");
    assert_eq!(texts(&log.derive_messages()), ["a", "b", "c", "d"]);

    let seq = log
        .append_replacing(user("<tóm tắt b c>"), 1, 3, T)
        .expect("replace");

    assert_eq!(texts(&log.derive_messages()), ["a", "<tóm tắt b c>", "d"]);
    // Bản ghi vẫn nguyên: dải bị che chỉ biến mất khỏi phép chiếu.
    assert_eq!(log.len(), 5);
    assert_eq!(
        texts(&[log
            .get(1)
            .and_then(SessionEventEnvelope::message)
            .cloned()
            .expect("b")]),
        ["b"]
    );
    assert_eq!(
        texts(&[log
            .get(2)
            .and_then(SessionEventEnvelope::message)
            .cloned()
            .expect("c")]),
        ["c"]
    );
    // Dấu vết: mọi node bị che đều được kê tên.
    assert_eq!(
        log.get(seq).expect("replace").source_event_seqs.as_deref(),
        Some(&[1, 2][..])
    );
    assert_eq!(log.surface().generation(), 1);
}

#[test]
fn replace_long_nhau_che_ca_ban_tom_tat_truoc() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    log.append_surface(assistant("b"), T).expect("b");
    log.append_surface(user("c"), T).expect("c");
    log.append_surface(assistant("d"), T).expect("d");
    let first = log
        .append_replacing(user("<tóm tắt 1>"), 1, 3, T)
        .expect("replace 1");

    // Node hiện tại: [0, first, 3]. Che hai node đầu, tức nuốt cả bản tóm tắt trước.
    let second = log
        .append_replacing(user("<tóm tắt 2>"), 0, 2, T)
        .expect("replace 2");

    assert_eq!(texts(&log.derive_messages()), ["<tóm tắt 2>", "d"]);
    assert_eq!(
        log.get(second)
            .expect("replace 2")
            .source_event_seqs
            .as_deref(),
        Some(&[0, first][..]),
        "replace lồng phải kê chính bản tóm tắt mà nó nuốt"
    );
    assert_eq!(log.len(), 6);
    assert_eq!(log.surface().generation(), 2);
}

#[test]
fn message_rong_bi_loai_khoi_lich_su_nhung_van_nam_trong_so() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    let rong = log
        .append_surface(assistant_rong(), T)
        .expect("assistant rỗng");
    log.append_surface(user("c"), T).expect("c");

    assert_eq!(texts(&log.derive_messages()), ["a", "c"]);
    assert_eq!(log.surface().nodes().len(), 3, "nó vẫn là một node surface");
    assert_eq!(log.len(), 3, "và vẫn là một dòng trong sổ");
    assert!(log.get(rong).expect("có mặt").message().is_none());
}

#[test]
fn su_kien_surface_bat_buoc_co_surface_op() {
    let mut log = SessionLog::new();
    match log.append(user("a"), T) {
        Err(SessionError::SurfaceOpRequired("user/message")) => {}
        other => panic!("phải đòi surface_op, gặp {:?}", other.err()),
    }
}

#[test]
fn su_kien_log_only_khong_duoc_mang_surface_op() {
    let mut log = SessionLog::new();
    match log.append_surface(SessionEvent::TurnStart(TurnStart { turn: 0 }), T) {
        Err(SessionError::SurfaceOpForbidden(_)) => {}
        other => panic!("phải cấm surface_op, gặp {:?}", other.err()),
    }
}

#[test]
fn replace_ngoai_pham_vi_bi_tu_choi() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    match log.append_replacing(user("x"), 0, 5, T) {
        Err(SessionError::SurfaceRangeOutOfBounds {
            start: 0,
            end: 5,
            len: 1,
        }) => {}
        other => panic!("phải là SurfaceRangeOutOfBounds, gặp {:?}", other.err()),
    }
    assert_eq!(log.len(), 1, "yêu cầu hỏng không được để lại vết trong sổ");
}

#[test]
fn replace_khong_ke_du_node_bi_che_bi_tu_choi() {
    let mut surface = pai_session::Surface::default();
    surface.apply(0, SurfaceOp::Append, None).expect("node 0");
    surface.apply(1, SurfaceOp::Append, None).expect("node 1");

    match surface.apply(2, SurfaceOp::Replace { start: 0, end: 2 }, Some(&[0])) {
        Err(SessionError::UncitedShadow { missing }) => assert_eq!(missing, vec![1]),
        other => panic!("phải là UncitedShadow, gặp {:?}", other.err()),
    }
}

#[test]
fn tool_result_la_surface_con_tool_call_thi_khong() {
    let call = SessionEvent::ToolCall(pai_session::ToolCall {
        turn: 0,
        step: 0,
        call_id: "c1".into(),
        name: "read".into(),
        arguments: r#"{"file_path":"a.rs"}"#.into(),
    });
    let result = SessionEvent::ToolResult(ToolResult {
        turn: 0,
        step: 0,
        call_id: "c1".into(),
        message: Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                call_id: "c1".into(),
                content: "…".into(),
                is_error: false,
            }],
            source: None,
        },
        error: None,
        meta: None,
    });
    assert!(!call.is_surface());
    assert!(result.is_surface());
}

#[test]
fn envelope_di_qua_json_roi_ve_nguyen_ven() {
    let mut log = SessionLog::new();
    log.append_surface(user("a"), T).expect("a");
    log.append_surface(assistant("b"), T).expect("b");
    log.append_replacing(user("<tóm tắt>"), 0, 2, T)
        .expect("replace");

    for envelope in log.events() {
        let text = serde_json::to_string(envelope).expect("mã hoá");
        let back: SessionEventEnvelope = serde_json::from_str(&text).expect("giải mã");
        assert_eq!(&back, envelope);
    }
}

#[test]
fn surface_op_tren_day_dung_hinh_dang_cua_dsh() {
    assert_eq!(
        serde_json::to_string(&SurfaceOp::Append).expect("mã hoá"),
        r#""append""#
    );
    assert_eq!(
        serde_json::to_string(&SurfaceOp::Replace { start: 1, end: 3 }).expect("mã hoá"),
        r#"{"op":"replace","start":1,"end":3}"#
    );
}
