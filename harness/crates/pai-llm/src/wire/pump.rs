//! Pump: joins a live HTTP request to a frame decoder and yields a `Stream`.
//! Both adapters need the same state machine and differ only in decoding, so that step
//! is a trait; written with `futures::stream::unfold` to avoid an `async-stream` dependency.

use std::collections::VecDeque;

use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};

use crate::error::{LlmError, LlmErrorCode};
use crate::stream::StreamChunk;

/// Turns one protocol's bytes into [`StreamChunk`]s; splitting this from HTTP is what lets the tests run with no network.
pub trait FrameDecoder: Send {
    /// Eat a byte slice and push chunks into `out`; `Err` only for a broken protocol, never for one unreadable line.
    fn push(&mut self, bytes: &[u8], out: &mut Vec<StreamChunk>) -> Result<(), LlmError>;

    /// The byte stream closed. Last chance to flush the buffer and close open blocks.
    fn finish(&mut self, out: &mut Vec<StreamChunk>);

    /// Has `Finish` been emitted? The pump uses this to tell "model finished" from "connection dropped mid-sentence".
    fn saw_finish(&self) -> bool;
}

/// Pump state.
enum Pump<D> {
    Connecting {
        request: BoxFuture<'static, Result<reqwest::Response, LlmError>>,
        decoder: D,
    },
    Reading {
        body: BoxStream<'static, Result<Vec<u8>, LlmError>>,
        decoder: D,
        queue: VecDeque<StreamChunk>,
    },
    /// Reading is done; chunks remain queued, and possibly a trailing error.
    Draining {
        queue: VecDeque<StreamChunk>,
        tail: Option<LlmError>,
    },
    Done,
}

/// Run a streaming request through a decoder; the stream ends with exactly one `Finish` or an `Err`, never in silence.
pub fn pump<D>(
    request: BoxFuture<'static, Result<reqwest::Response, LlmError>>,
    decoder: D,
) -> BoxStream<'static, Result<StreamChunk, LlmError>>
where
    D: FrameDecoder + 'static,
{
    futures::stream::unfold(Pump::Connecting { request, decoder }, |state| async move {
        let mut state = state;
        loop {
            match state {
                Pump::Connecting { request, decoder } => match request.await {
                    Err(err) => return Some((Err(err), Pump::Done)),
                    Ok(response) => {
                        let status = response.status();
                        if !status.is_success() {
                            // Read the error body before discarding it: it is the only thing separating "context window exceeded" from "bad request".
                            let body = response.text().await.unwrap_or_default();
                            let err = LlmError::from_status(status.as_u16(), &body);
                            return Some((Err(err), Pump::Done));
                        }
                        let body = response
                            .bytes_stream()
                            // Copy into `Vec<u8>` so `bytes`'s type stays out of the crate interface; one copy per socket chunk is negligible.
                            .map(|item| item.map(|bytes| bytes.to_vec()).map_err(LlmError::from))
                            .boxed();
                        state = Pump::Reading {
                            body,
                            decoder,
                            queue: VecDeque::new(),
                        };
                    }
                },
                Pump::Reading {
                    body,
                    decoder,
                    queue,
                } => {
                    let mut body = body;
                    let mut decoder = decoder;
                    let mut queue = queue;
                    if let Some(chunk) = queue.pop_front() {
                        return Some((
                            Ok(chunk),
                            Pump::Reading {
                                body,
                                decoder,
                                queue,
                            },
                        ));
                    }
                    match body.next().await {
                        Some(Ok(bytes)) => {
                            let mut out = Vec::new();
                            if let Err(err) = decoder.push(&bytes, &mut out) {
                                // Protocol error: flush what was assembled before reporting, so the received answer is not thrown away.
                                queue.extend(out);
                                state = Pump::Draining {
                                    queue,
                                    tail: Some(err),
                                };
                                continue;
                            }
                            queue.extend(out);
                            state = Pump::Reading {
                                body,
                                decoder,
                                queue,
                            };
                        }
                        Some(Err(err)) => {
                            state = Pump::Draining {
                                queue,
                                tail: Some(err),
                            };
                        }
                        None => {
                            let mut out = Vec::new();
                            decoder.finish(&mut out);
                            queue.extend(out);
                            let tail = (!decoder.saw_finish()).then(|| {
                                LlmError::new(
                                    LlmErrorCode::ProviderUnavailable,
                                    "kết nối đóng trước khi mô hình báo dừng",
                                )
                            });
                            state = Pump::Draining { queue, tail };
                        }
                    }
                }
                Pump::Draining { queue, tail } => {
                    let mut queue = queue;
                    if let Some(chunk) = queue.pop_front() {
                        return Some((Ok(chunk), Pump::Draining { queue, tail }));
                    }
                    return tail.map(|err| (Err(err), Pump::Done));
                }
                Pump::Done => return None,
            }
        }
    })
    .boxed()
}

/// A stream carrying exactly one error. Used when the request could not even be built.
pub fn failed(err: LlmError) -> BoxStream<'static, Result<StreamChunk, LlmError>> {
    futures::stream::once(async move { Err(err) }).boxed()
}
