//! Tầng dây, kiểm trên chuỗi byte cố định — **không cần mạng**.
//!
//! Mọi bài ở đây mô phỏng cùng một sự thật: một lần đọc socket không phải một đơn vị của
//! giao thức. Điểm cắt được đặt vào đúng những chỗ khó nhất: giữa một event SSE, giữa một
//! dòng NDJSON, giữa `\r` và `\n`, và giữa hai byte của một ký tự tiếng Việt.

use pai_llm::assembler::BlockAssembler;
use pai_llm::stream::{FinishReason, StreamChunk};
use pai_llm::wire::pump::FrameDecoder;
use pai_llm::wire::{LineDecoder, SseDecoder};

/// Đẩy một thân phản hồi qua bộ giải mã, cắt thành từng lát `size` byte.
///
/// Cắt theo **byte**, không theo ký tự: đó chính là điều TCP làm, và là điều mọi bộ giải
/// mã đệm chuỗi thay vì đệm byte sẽ hỏng.
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
    // Nửa đầu: chưa có dòng trống, nên **chưa được phát gì cả**.
    assert!(decoder.push(b"data: {\"a\":1").is_empty());
    assert!(decoder.push(b"}").is_empty());
    assert!(decoder.push(b"\n").is_empty());
    // Dòng trống mới là thứ phát ra event.
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

/// Điểm cắt rơi vào **giữa một ký tự UTF-8 nhiều byte**.
///
/// "chào" và "🌍" chiếm nhiều byte; cắt mỗi 5 byte đảm bảo có lát dừng giữa chừng. Bộ
/// giải mã đệm byte nên ghép lại đúng; nếu nó đệm `String` thì chỗ này ra dấu hỏi.
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

/// Ollama gửi tool call nguyên khối với `arguments` là object, và vẫn báo
/// `done_reason: "stop"`. Lý do dừng phải là `ToolCalls`, nếu không vòng lặp agent sẽ
/// kết thúc lượt trong lúc mô hình đang chờ kết quả.
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

/// Ollama báo lỗi *bên trong* luồng, với HTTP 200 ở ngoài.
#[test]
fn ollama_loi_giua_luong_thanh_err() {
    let mut decoder = pai_llm::ollama::ChatDecoder::new();
    let mut out = Vec::new();
    let err = decoder
        .push(b"{\"error\":\"model 'khong-co' not found\"}\n", &mut out)
        .expect_err("phải là lỗi");
    assert!(err.message.contains("khong-co"));
}

/// Kết nối đứt giữa câu: bộ giải mã **không** được giả vờ là đã xong.
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

/// Tham số tool nhỏ giọt qua nhiều event SSE, và cả thân phản hồi bị cắt thành từng lát
/// byte tuỳ ý. Hai tầng cắt vụn chồng lên nhau — đúng như trên dây thật.
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
        // Mảnh thứ hai kết thúc bằng `\"a\` — cắt ngay giữa một escape.
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

/// Máy chủ đóng kết nối sau `finish_reason` mà không gửi `[DONE]` — vẫn là một lượt xong.
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
