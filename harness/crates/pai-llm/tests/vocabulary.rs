//! Block assembler: folding a chunk stream into a message.
//! The most important test in the crate - a wrong assembler breaks every tool call,
//! and does it silently.

use pai_llm::assembler::BlockAssembler;
use pai_llm::message::{ContentBlock, Message};
use pai_llm::stream::{BlockKind, FinishReason, StreamChunk, TokenUsage};

fn tool_delta(index: u32, arguments: &str) -> StreamChunk {
    StreamChunk::ToolCallDelta {
        index,
        id: None,
        name: None,
        arguments: arguments.to_string(),
    }
}

/// Tool arguments arrive in ten fragments, with one split landing inside an escaped `\"` - the real shape of OpenAI streaming, where per-fragment parsing fails.
#[test]
fn tham_so_tool_rap_tu_nhieu_manh() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::BlockStart {
        index: 0,
        kind: BlockKind::ToolUse,
    });
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 0,
        id: Some("call_abc".into()),
        name: Some("read".into()),
        arguments: String::new(),
    });
    // Target string: {"path":"bao/cao \"quy 4\".md","dong":12}
    for piece in [
        "{\"pa",
        "th\":\"bao/cao ",
        // The split falls between the backslash and the quote of the escape.
        "\\",
        "\"quy 4\\",
        "\".md\",",
        "\"dong\":1",
        "2}",
    ] {
        assembler.push(&tool_delta(0, piece));
    }
    assembler.push(&StreamChunk::BlockEnd { index: 0 });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::ToolCalls,
    });

    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_abc");
    assert_eq!(calls[0].name, "read");
    assert_eq!(
        calls[0].arguments,
        "{\"path\":\"bao/cao \\\"quy 4\\\".md\",\"dong\":12}"
    );

    let parsed = calls[0]
        .parse_arguments()
        .expect("tham số đã ráp phải là JSON hợp lệ");
    assert_eq!(parsed["path"], "bao/cao \"quy 4\".md");
    assert_eq!(parsed["dong"], 12);
}

/// Two parallel tool calls with interleaved deltas, exactly as OpenAI streams multi-tool turns.
#[test]
fn hai_tool_call_xen_ke_khong_lan_nhau() {
    let mut assembler = BlockAssembler::new();
    for index in [0, 1] {
        assembler.push(&StreamChunk::BlockStart {
            index,
            kind: BlockKind::ToolUse,
        });
    }
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 0,
        id: Some("a".into()),
        name: Some("glob".into()),
        arguments: "{\"pat".into(),
    });
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 1,
        id: Some("b".into()),
        name: Some("grep".into()),
        arguments: "{\"q\":".into(),
    });
    assembler.push(&tool_delta(0, "tern\":\"*.rs\"}"));
    assembler.push(&tool_delta(1, "\"fn main\"}"));
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::ToolCalls,
    });

    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name, "glob");
    assert_eq!(calls[0].arguments, "{\"pattern\":\"*.rs\"}");
    assert_eq!(calls[1].name, "grep");
    assert_eq!(calls[1].arguments, "{\"q\":\"fn main\"}");
}

#[test]
fn van_ban_va_suy_luan_thanh_hai_khoi_rieng() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::BlockStart {
        index: 0,
        kind: BlockKind::Reasoning,
    });
    assembler.push(&StreamChunk::ReasoningDelta {
        index: 0,
        text: "để xem".into(),
    });
    assembler.push(&StreamChunk::BlockEnd { index: 0 });
    assembler.push(&StreamChunk::BlockStart {
        index: 1,
        kind: BlockKind::Text,
    });
    assembler.push(&StreamChunk::TextDelta {
        index: 1,
        text: "Xin ".into(),
    });
    assembler.push(&StreamChunk::TextDelta {
        index: 1,
        text: "chào".into(),
    });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::Stop,
    });

    let message = assembler.message();
    assert_eq!(
        message,
        Message::Assistant {
            content: vec![
                ContentBlock::Reasoning {
                    text: "để xem".into()
                },
                ContentBlock::Text {
                    text: "Xin chào".into()
                },
            ]
        }
    );
    // `text()` returns the answer only: reasoning is not shown as the assistant speaking.
    assert_eq!(message.text(), "Xin chào");
}

/// Invariant: `Usage` precedes `Finish`, and nothing follows `Finish`.
#[test]
fn khong_gi_duoc_ghi_nhan_sau_finish() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::BlockStart {
        index: 0,
        kind: BlockKind::Text,
    });
    assembler.push(&StreamChunk::TextDelta {
        index: 0,
        text: "xong".into(),
    });
    assembler.push(&StreamChunk::Usage {
        usage: TokenUsage {
            input_tokens: 11,
            output_tokens: 3,
            total_tokens: None,
        },
    });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::Stop,
    });
    // The server breaks the rule.
    assembler.push(&StreamChunk::TextDelta {
        index: 0,
        text: " thêm".into(),
    });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::Length,
    });

    assert_eq!(assembler.text(), "xong");
    assert_eq!(assembler.finish_reason(), Some(FinishReason::Stop));
    assert_eq!(assembler.usage().map(|u| u.total()), Some(14));
    assert!(assembler.is_finished());
}

/// A no-argument tool: OpenAI sends `"arguments": ""`, and an empty string does not parse.
#[test]
fn tham_so_rong_thanh_object_rong() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 0,
        id: Some("z".into()),
        name: Some("todo_write".into()),
        arguments: String::new(),
    });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::ToolCalls,
    });
    let calls = assembler.tool_calls();
    assert_eq!(calls[0].arguments, "{}");
    assert!(calls[0].parse_arguments().is_ok());
}

/// Ollama emits no tool call id, so the assembler must mint a stable one for the next round.
#[test]
fn thieu_id_thi_sinh_theo_index() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::ToolCallDelta {
        index: 3,
        id: None,
        name: Some("bash".into()),
        arguments: "{}".into(),
    });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::ToolCalls,
    });
    assert_eq!(assembler.tool_calls()[0].id, "call_3");
}

/// Empty text blocks must not reach the log: Ollama opens an empty message on the `done` line.
#[test]
fn khoi_rong_bi_loai() {
    let mut assembler = BlockAssembler::new();
    assembler.push(&StreamChunk::BlockStart {
        index: 0,
        kind: BlockKind::Text,
    });
    assembler.push(&StreamChunk::BlockEnd { index: 0 });
    assembler.push(&StreamChunk::Finish {
        reason: FinishReason::Stop,
    });
    assert_eq!(assembler.message(), Message::Assistant { content: vec![] });
}
