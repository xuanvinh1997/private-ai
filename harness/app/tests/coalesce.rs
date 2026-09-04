//! End-of-turn event ordering, locking a bug users saw: a stub assistant message with a cursor that never
//! stopped blinking, caused by `Final` overtaking buffered tokens. Two timing invariants the type system
//! cannot hold: non-token events never pass tokens, and the command returns only after the channel drains.

use std::sync::{Arc, Mutex};

use pai_app_lib::coalesce::Coalescer;
use pai_app_lib::protocol::AgentEvent;
use tauri::ipc::{Channel, InvokeResponseBody};

/// A channel that records everything passing through, as JSON strings.
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

    // No 16 ms deadline flush had time to run, so `finish` must drain the rest itself.
    bo_gop.send(AgentEvent::Token {
        text: "còn trong bộ đệm".into(),
    });
    bo_gop.finish().await;

    let seen = seen.lock().expect("khoá").clone();
    assert_eq!(seen.len(), 1, "bộ đệm bị bỏ quên: {seen:?}");
    assert!(seen[0].contains("còn trong bộ đệm"), "{seen:?}");
}
