//! Gộp token trước khi gửi qua IPC.
//!
//! Một lần vượt biên IPC của Tauri đắt hơn hẳn một signal của Qt. Mô hình chạy nhanh
//! phát ra hàng nghìn token mỗi phút, và gửi từng cái một sẽ nghẽn ở webview chứ không
//! ở mô hình. Gộp theo cửa sổ ~16 ms cho ra đúng một lần vẽ mỗi khung hình — mắt không
//! phân biệt được, còn máy thì rảnh hẳn.

use std::time::Duration;

use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::protocol::AgentEvent;

/// Một khung hình ở 60 Hz.
const WINDOW: Duration = Duration::from_millis(16);

/// Đầu vào của bộ gộp.
///
/// Chỉ `Token` được gộp. Mọi sự kiện khác đi thẳng, và **xả hết token đang chờ trước
/// khi đi** — nếu không, một thẻ tool sẽ nhảy lên trước đoạn văn sinh ra nó.
pub struct Coalescer {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl Coalescer {
    pub fn spawn(channel: Channel<AgentEvent>) -> Coalescer {
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut deadline: Option<tokio::time::Instant> = None;

            loop {
                let event = match deadline {
                    Some(at) => match tokio::time::timeout_at(at, rx.recv()).await {
                        Ok(event) => event,
                        Err(_) => {
                            flush(&channel, &mut buffer);
                            deadline = None;
                            continue;
                        }
                    },
                    None => rx.recv().await,
                };

                let Some(event) = event else {
                    flush(&channel, &mut buffer);
                    return;
                };

                match event {
                    AgentEvent::Token { text } => {
                        buffer.push_str(&text);
                        deadline.get_or_insert_with(|| tokio::time::Instant::now() + WINDOW);
                    }
                    other => {
                        flush(&channel, &mut buffer);
                        deadline = None;
                        if channel.send(other).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Coalescer { tx }
    }

    /// Gửi. Kênh đứt thì bỏ qua: lượt sẽ kết thúc theo đường huỷ, không phải ở đây.
    pub fn send(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }
}

fn flush(channel: &Channel<AgentEvent>, buffer: &mut String) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    let _ = channel.send(AgentEvent::Token { text });
}
