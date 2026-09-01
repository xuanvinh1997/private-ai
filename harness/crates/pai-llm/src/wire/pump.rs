//! Bơm: nối một request HTTP đang chạy với một bộ giải mã khung, cho ra một `Stream`.
//!
//! Cả hai adapter đều cần đúng một máy trạng thái: gửi request → kiểm mã trạng thái →
//! đọc byte → giải mã → nhả chunk. Chỉ bước "giải mã" là khác nhau, nên nó là một trait
//! và phần còn lại viết một lần.
//!
//! Viết bằng `futures::stream::unfold` chứ không `async-stream`: thêm một dependency chỉ
//! để có cú pháp `yield` là không đáng, và máy trạng thái viết tay ở đây đủ nhỏ để đọc.

use std::collections::VecDeque;

use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};

use crate::error::{LlmError, LlmErrorCode};
use crate::stream::StreamChunk;

/// Biến byte của một giao thức cụ thể thành [`StreamChunk`].
///
/// Tách khỏi phần HTTP là điều khiến bộ test chạy được **không cần mạng**: bài test khó
/// nhất — điểm cắt rơi vào giữa một event — chỉ cần gọi `push` với hai lát byte.
pub trait FrameDecoder: Send {
    /// Ăn một mảnh byte và đẩy chunk vào `out`.
    ///
    /// Trả `Err` chỉ khi chính giao thức hỏng (máy chủ trả một object lỗi giữa luồng).
    /// JSON không đọc được ở một dòng lẻ thì bỏ dòng đó, không giết cả luồng.
    fn push(&mut self, bytes: &[u8], out: &mut Vec<StreamChunk>) -> Result<(), LlmError>;

    /// Luồng byte đã đóng. Cơ hội cuối để nhả phần đệm dở và đóng các khối còn mở.
    fn finish(&mut self, out: &mut Vec<StreamChunk>);

    /// Đã phát ra `Finish` chưa. Bơm dựa vào đây để phân biệt "mô hình nói xong" với
    /// "kết nối đứt giữa câu" — hai thứ trông y hệt nhau ở tầng TCP.
    fn saw_finish(&self) -> bool;
}

/// Trạng thái của bơm.
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
    /// Đã đọc hết; còn chunk trong hàng đợi, và có thể còn một lỗi ở chót.
    Draining {
        queue: VecDeque<StreamChunk>,
        tail: Option<LlmError>,
    },
    Done,
}

/// Chạy một request streaming qua một bộ giải mã.
///
/// Bất biến giữ ở đây: luồng kết thúc bằng **đúng một** `Finish`, hoặc bằng một `Err`.
/// Không có khả năng thứ ba, nên người gọi không cần đoán xem im lặng nghĩa là gì.
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
                            // Đọc thân lỗi trước khi bỏ: nó là thứ duy nhất phân biệt
                            // "tràn cửa sổ ngữ cảnh" với "request sai".
                            let body = response.text().await.unwrap_or_default();
                            let err = LlmError::from_status(status.as_u16(), &body);
                            return Some((Err(err), Pump::Done));
                        }
                        let body = response
                            .bytes_stream()
                            // Sao chép sang `Vec<u8>` để khỏi phơi kiểu của `bytes` ra
                            // giao diện crate; một lần sao mỗi mảnh socket là không đáng kể.
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
                                // Lỗi giao thức: nhả nốt cái đã ráp rồi mới báo lỗi, để
                                // phần câu trả lời đã nhận không bị vứt đi.
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

/// Một luồng chỉ có đúng một lỗi. Dùng khi request còn chưa dựng nổi.
pub fn failed(err: LlmError) -> BoxStream<'static, Result<StreamChunk, LlmError>> {
    futures::stream::once(async move { Err(err) }).boxed()
}
