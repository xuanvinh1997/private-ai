//! Bộ ráp khối: gấp một luồng chunk thành một message.
//!
//! Bài quan trọng nhất của cả crate. Nếu bộ ráp sai thì mọi tool call đều sai, và nó sai
//! im lặng — mô hình xin gọi `read(path="a\"b")` mà ta gọi `read(path="a")`.

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

/// Tham số tool đến làm mười mảnh, và **một điểm cắt rơi vào giữa một escape `\"`**.
///
/// Đây là hình dạng thật của OpenAI streaming. Mọi cách cài đặt cố parse từng mảnh đều
/// hỏng ở đúng chỗ này: mảnh `{"path":"a\` không phải JSON hợp lệ, và cũng không phải
/// một chuỗi đã kết thúc.
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
    // Chuỗi đích: {"path":"bao/cao \"quy 4\".md","dong":12}
    for piece in [
        "{\"pa",
        "th\":\"bao/cao ",
        // Điểm cắt nằm giữa dấu chéo ngược và dấu nháy kép của escape.
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

/// Hai tool call song song, delta xen kẽ nhau. OpenAI làm đúng như vậy khi mô hình xin
/// gọi nhiều tool trong một lượt.
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
    // `text()` chỉ trả câu trả lời: suy luận không phải thứ hiện ra như lời của trợ lý.
    assert_eq!(message.text(), "Xin chào");
}

/// Bất biến: `Usage` đứng trước `Finish`, và **không gì đứng sau `Finish`**.
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
    // Máy chủ phá luật.
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

/// Tool không tham số: OpenAI gửi `"arguments": ""`, mà chuỗi rỗng không parse được.
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

/// Ollama không phát id tool call. Bộ ráp phải sinh một cái ổn định để lượt sau đối chiếu.
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

/// Khối văn bản rỗng không được lọt vào sổ tay: Ollama mở một message rỗng ở dòng `done`.
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
