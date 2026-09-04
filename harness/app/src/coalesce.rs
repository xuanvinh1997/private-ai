//! Coalescing tokens before they cross IPC. A Tauri IPC hop is far pricier than a Qt signal, and a fast
//! model emits thousands of tokens a minute, so per-token sends bottleneck in the webview.
//! A ~16 ms window yields one repaint per frame, which the eye cannot tell apart.

use std::time::Duration;

use tauri::ipc::Channel;
use tokio::sync::mpsc;

use crate::protocol::AgentEvent;

/// One frame at 60 Hz.
const WINDOW: Duration = Duration::from_millis(16);

/// The coalescer's input. Only `Token` is merged; every other event flushes the pending tokens first, or a
/// tool card would jump ahead of the prose that produced it.
pub struct Coalescer {
    tx: mpsc::UnboundedSender<AgentEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl Coalescer {
    pub fn spawn(channel: Channel<AgentEvent>) -> Coalescer {
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let task = tokio::spawn(async move {
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
        Coalescer { tx, task }
    }

    /// Send; a broken channel is ignored, since the turn ends via cancellation, not here.
    pub fn send(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }

    /// Flush everything before returning, so that a command returning means every event of the turn has left
    /// the channel; otherwise late buffered tokens arrive after the UI closed the block and spawn a stub message.
    pub async fn finish(self) {
        // Drop `tx` so the loop sees the channel close, flushes the buffer and exits.
        drop(self.tx);
        let _ = self.task.await;
    }
}

fn flush(channel: &Channel<AgentEvent>, buffer: &mut String) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    let _ = channel.send(AgentEvent::Token { text });
}
