//! Wire layer, checked against fixed byte strings - no network needed.
//! Every case models one fact: a socket read is not a protocol unit, so splits land in
//! the hardest places - mid SSE event, mid NDJSON line, between `\r` and `\n`, mid UTF-8.

use pai_llm::assembler::BlockAssembler;
use pai_llm::stream::{FinishReason, StreamChunk};
use pai_llm::wire::pump::FrameDecoder;
use pai_llm::wire::{LineDecoder, SseDecoder};

/// Push a response body through a decoder in `size`-byte slices; sliced by *byte*, exactly as TCP does.
fn feed<D: FrameDecoder>(decoder: &mut D, body: &[u8], size: usize) -> Vec<StreamChunk> {
    let mut out = Vec::new();
    for slice in body.chunks(size) {
        decoder
            .push(slice, &mut out)
            .expect("không có lỗi giao thức");
    }
    decoder.finish(&mut out);
    out
}

// --- SSE ----------------------------------------------------------------------------

#[test]
fn sse_bi_cat_giua_event() {
    let mut decoder = SseDecoder::new();
    // First half: no blank line yet, so nothing may be emitted.
    assert!(decoder.push(b"data: {\"a\":1").is_empty());
    assert!(decoder.push(b"}").is_empty());
    assert!(decoder.push(b"\n").is_empty());
    // The blank line is what emits the event.
    let events = decoder.push(b"\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "{\"a\":1}");
}

#[test]
fn sse_bi_cat_giua_crlf() {
    let mut decoder = SseDecoder::new();
    assert!(decoder.push(b"data: xin chao\r").is_empty());
    let events = decoder.push(b"\n\r\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "xin chao");
}

#[test]
fn sse_bo_qua_chu_thich_va_gop_nhieu_dong_data() {
    let mut decoder = SseDecoder::new();
    let events =
        decoder.push(b": keep-alive\nevent: message\ndata: mot\ndata: hai\n\ndata: [DONE]\n\n");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].name.as_deref(), Some("message"));
    assert_eq!(events[0].data, "mot\nhai");
    assert!(events[1].is_done());
}

#[test]
fn sse_dong_trong_thua_khong_sinh_event_rong() {
    let mut decoder = SseDecoder::new();
    assert!(decoder.push(b"\n\n\n").is_empty());
    assert_eq!(decoder.push(b"data: x\n\n").len(), 1);
}

// --- NDJSON -------------------------------------------------------------------------

#[test]
fn ndjson_bi_cat_giua_dong() {
    let mut decoder = LineDecoder::new();
    assert!(decoder.push(b"{\"done\":fal").is_empty());
    assert!(decoder.push(b"se}").is_empty());
    assert_eq!(
        decoder.push(b"\n{\"done\":true}\n"),
        vec!["{\"done\":false}", "{\"done\":true}"]
    );
}

// --- Ollama /api/chat ---------------------------------------------------------------

/// A split landing inside a multi-byte UTF-8 character; a decoder that buffered `String` instead of bytes would produce replacement characters here.
#[test]
fn ollama_ky_tu_nhieu_byte_bi_cat_giua_chung() {
    let body = concat!(
        "{\"message\":{\"role\":\"assistant\",\"content\":\"Xin chào\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\" thế giới 🌍\"},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",",
        "\"prompt_eval_count\":9,\"eval_count\":4}\n"
    )
    .as_bytes();

    for size in [1, 3, 5, 7, 13, body.len()] {
        let mut decoder = pai_llm::ollama::ChatDecoder::new();
        let chunks = feed(&mut decoder, body, size);
        let mut assembler = BlockAssembler::new();
        for chunk in &chunks {
            assembler.push(chunk);
        }
        assert_eq!(assembler.text(), "Xin chào thế giới 🌍", "lát {size} byte");
        assert_eq!(assembler.finish_reason(), Some(FinishReason::Stop));
        assert_eq!(assembler.usage().map(|u| u.total()), Some(13));
        assert!(decoder.saw_finish());
    }
}

/// Ollama sends whole tool calls yet still reports `done_reason: "stop"`, so the finish reason must be `ToolCalls` or the agent loop ends the turn early.
#[test]
fn ollama_tool_call_doi_ly_do_dung() {
    let body = concat!(
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[",
        "{\"function\":{\"name\":\"read\",\"arguments\":{\"path\":\"a\\\"b.md\"}}}]},\"done\":false}\n",
        "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n"
    )
    .as_bytes();
    let mut decoder = pai_llm::ollama::ChatDecoder::new();
    let chunks = feed(&mut decoder, body, 4);
    let mut assembler = BlockAssembler::new();
    for chunk in &chunks {
        assembler.push(chunk);
    }
    assert_eq!(assembler.finish_reason(), Some(FinishReason::ToolCalls));
    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "read");
    assert_eq!(
        calls[0].parse_arguments().expect("JSON hợp lệ")["path"],
        "a\"b.md"
    );
}

/// Ollama reports errors *inside* the stream, with HTTP 200 outside.
#[test]
fn ollama_loi_giua_luong_thanh_err() {
    let mut decoder = pai_llm::ollama::ChatDecoder::new();
    let mut out = Vec::new();
    let err = decoder
        .push(b"{\"error\":\"model 'khong-co' not found\"}\n", &mut out)
        .expect_err("phải là lỗi");
    assert!(err.message.contains("khong-co"));
}

/// Connection dropped mid-sentence: the decoder must not pretend the turn finished.
#[test]
fn ollama_dut_giua_chung_khong_bao_da_xong() {
    let mut decoder = pai_llm::ollama::ChatDecoder::new();
    let chunks = feed(
        &mut decoder,
        b"{\"message\":{\"content\":\"nua cau\"},\"done\":false}\n",
        6,
    );
    assert!(!decoder.saw_finish());
    assert!(!chunks.iter().any(StreamChunk::is_finish));
}

// --- OpenAI /v1/chat/completions ----------------------------------------------------

/// Tool arguments dripped across SSE events while the body is also sliced into arbitrary bytes - two layers of fragmentation, as on the real wire.
#[test]
fn openai_tham_so_tool_rap_qua_nhieu_event() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",",
        "\"type\":\"function\",\"function\":{\"name\":\"edit\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
        "\"function\":{\"arguments\":\"{\\\"cu\\\":\\\"a\\\\\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
        "\"function\":{\"arguments\":\"\\\"b\\\",\\\"moi\\\":\\\"c\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],",
        "\"usage\":{\"prompt_tokens\":30,\"completion_tokens\":8,\"total_tokens\":38}}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes();

    for size in [1, 9, 64, body.len()] {
        let mut decoder = pai_llm::openai::ChatDecoder::new();
        let chunks = feed(&mut decoder, body, size);
        let mut assembler = BlockAssembler::new();
        for chunk in &chunks {
            assembler.push(chunk);
        }
        let calls = assembler.tool_calls();
        assert_eq!(calls.len(), 1, "lát {size} byte");
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "edit");
        // The second fragment ends with `\"a\` - split inside an escape.
        assert_eq!(calls[0].arguments, "{\"cu\":\"a\\\"b\",\"moi\":\"c\"}");
        let parsed = calls[0].parse_arguments().expect("JSON hợp lệ sau khi ráp");
        assert_eq!(parsed["cu"], "a\"b");
        assert_eq!(parsed["moi"], "c");
        assert_eq!(assembler.finish_reason(), Some(FinishReason::ToolCalls));
        assert_eq!(assembler.usage().map(|u| u.total()), Some(38));
    }
}

#[test]
fn openai_van_ban_thuong_va_thu_tu_usage_truoc_finish() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Chào \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"bạn\"},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes();
    let mut decoder = pai_llm::openai::ChatDecoder::new();
    let chunks = feed(&mut decoder, body, 11);

    let usage_at = chunks
        .iter()
        .position(|c| matches!(c, StreamChunk::Usage { .. }));
    let finish_at = chunks.iter().position(StreamChunk::is_finish);
    assert!(usage_at < finish_at, "Usage phải đứng trước Finish");
    assert_eq!(
        finish_at,
        Some(chunks.len() - 1),
        "Finish phải là chunk cuối"
    );

    let mut assembler = BlockAssembler::new();
    for chunk in &chunks {
        assembler.push(chunk);
    }
    assert_eq!(assembler.text(), "Chào bạn");
}

/// The server closes after `finish_reason` without sending `[DONE]`; still a finished turn.
#[test]
fn openai_thieu_done_van_dong_luong_tu_te() {
    let body =
        b"data: {\"choices\":[{\"delta\":{\"content\":\"xong\"},\"finish_reason\":\"stop\"}]}\n\n";
    let mut decoder = pai_llm::openai::ChatDecoder::new();
    let chunks = feed(&mut decoder, body, 5);
    assert!(decoder.saw_finish());
    assert!(chunks.last().is_some_and(StreamChunk::is_finish));
}

#[test]
fn openai_suy_luan_tach_khoi_cau_tra_loi() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"nghĩ đã\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"rồi\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    )
    .as_bytes();
    let mut decoder = pai_llm::openai::ChatDecoder::new();
    let mut assembler = BlockAssembler::new();
    for chunk in feed(&mut decoder, body, 7) {
        assembler.push(&chunk);
    }
    assert_eq!(assembler.text(), "rồi");
    assert_eq!(assembler.blocks().len(), 2);
}
