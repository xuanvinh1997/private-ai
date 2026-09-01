//! Thứ tự sự kiện cuối lượt.
//!
//! Bài này khoá lại một lỗi đã hiện lên màn hình người dùng: một tin nhắn trợ lý cụt,
//! mang con trỏ nhấp nháy không bao giờ tắt. Nguyên nhân là thứ tự — `Final` gửi thẳng
//! vào kênh trong khi token cuối còn nằm trong bộ đệm 16 ms, nên giao diện đóng khối trả
//! lời rồi mới nhận token, và token muộn đẻ ra một khối mới không ai đóng.
//!
//! Hai bất biến, và cả hai đều là bất biến **về thời gian**, thứ mà hệ kiểu không giữ hộ:
//! sự kiện không phải token không bao giờ vượt lên trước token đứng trước nó, và lệnh chỉ
//! trả về sau khi kênh đã nhận hết.

use std::sync::{Arc, Mutex};

use pai_app_lib::coalesce::Coalescer;
use pai_app_lib::protocol::AgentEvent;
use tauri::ipc::{Channel, InvokeResponseBody};

/// Kênh thu lại mọi thứ đi qua, dưới dạng chuỗi JSON.
fn thu() -> (Channel<AgentEvent>, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let channel = Channel::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Json(text) = body {
            sink.lock().expect("khoá").push(text);
        }
        Ok(())
    });
    (channel, seen)
}

#[tokio::test]
async fn final_khong_vuot_len_truoc_token_cuoi() {
    let (channel, seen) = thu();
    let bo_gop = Coalescer::spawn(channel);

    bo_gop.send(AgentEvent::Token {
        text: "hôm ".into(),
    });
    bo_gop.send(AgentEvent::Token {
        text: "nay?".into(),
    });
    bo_gop.send(AgentEvent::Final {
        message_id: "m1".into(),
    });
    bo_gop.finish().await;

    let seen = seen.lock().expect("khoá").clone();
    assert_eq!(
        seen.len(),
        2,
        "đúng hai lần gửi: một cụm token, một final: {seen:?}"
    );
    assert!(
        seen[0].contains("hôm nay?"),
        "token phải được gộp và đi trước: {seen:?}"
    );
    assert!(
        seen[1].contains("final"),
        "final phải đi sau token cuối: {seen:?}"
    );
}

#[tokio::test]
async fn finish_tra_ve_sau_khi_kenh_da_nhan_het() {
    let (channel, seen) = thu();
    let bo_gop = Coalescer::spawn(channel);

    // Không `flush` nào kịp chạy theo thời hạn 16 ms: `finish` phải tự xả nốt.
    bo_gop.send(AgentEvent::Token {
        text: "còn trong bộ đệm".into(),
    });
    bo_gop.finish().await;

    let seen = seen.lock().expect("khoá").clone();
    assert_eq!(seen.len(), 1, "bộ đệm bị bỏ quên: {seen:?}");
    assert!(seen[0].contains("còn trong bộ đệm"), "{seen:?}");
}
