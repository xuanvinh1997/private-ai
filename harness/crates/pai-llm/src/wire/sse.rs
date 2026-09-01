//! Bộ phân tích Server-Sent Events, viết tay.
//!
//! Không dùng crate có sẵn vì phần SSE mà một máy chủ OpenAI-compatible thực sự dùng chỉ
//! là ba dòng luật, còn cái ta cần kiểm soát chặt — điểm cắt giữa hai lần đọc socket —
//! thì crate nào cũng giấu đi. Sáu chục dòng đổi lấy quyền viết bài test cho đúng chỗ
//! hay hỏng là một món hời.
//!
//! Luật cài đặt (theo WHATWG, đã lược phần không máy chủ nào dùng):
//!
//! - Dòng bắt đầu bằng `:` là chú thích. Một số proxy gửi `: keep-alive` định kỳ.
//! - `field: value`; một dấu cách sau dấu hai chấm bị bỏ. Dòng không có `:` là field rỗng.
//! - Nhiều dòng `data:` trong một event được nối bằng `\n`.
//! - **Dòng trống mới là thứ phát ra event.** Đây là chỗ cắt giữa chừng gây hại: đọc
//!   xong `data: {...}` mà chưa thấy dòng trống thì chưa được phát gì cả.
//! - `id:` và `retry:` bị bỏ qua: ở đây không có kết nối lại, huỷ là thả stream.

use crate::wire::ndjson::LineDecoder;

/// Một event đã trọn vẹn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    /// Trường `event:`. Máy chủ OpenAI-compatible hầu như không gửi, nhưng vài bản có.
    pub name: Option<String>,
    /// Các dòng `data:` đã nối.
    pub data: String,
}

impl SseEvent {
    /// Dấu hiệu kết thúc luồng của OpenAI. Không phải JSON, nên phải chặn trước khi parse.
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Bộ giải mã có trạng thái, ăn byte và nhả event.
#[derive(Debug, Default)]
pub struct SseDecoder {
    lines: LineDecoder,
    data: String,
    name: Option<String>,
    has_data: bool,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ăn một mảnh byte, trả về mọi event đã trọn vẹn.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for line in self.lines.push(bytes) {
            if let Some(event) = self.line(&line) {
                events.push(event);
            }
        }
        events
    }

    /// Event dở dang khi luồng đóng mà thiếu dòng trống cuối. Vài máy chủ đóng kết nối
    /// ngay sau `data: [DONE]` và không gửi `\n\n`.
    pub fn flush(&mut self) -> Option<SseEvent> {
        if let Some(rest) = self.lines.flush()
            && let Some(event) = self.line(&rest)
        {
            return Some(event);
        }
        self.dispatch()
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.name = Some(value.to_string()),
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.has_data {
            // Dòng trống thừa giữa hai event: không phát event rỗng, chỉ dọn `event:` lẻ.
            self.name = None;
            return None;
        }
        let event = SseEvent {
            name: self.name.take(),
            data: std::mem::take(&mut self.data),
        };
        self.has_data = false;
        Some(event)
    }
}
