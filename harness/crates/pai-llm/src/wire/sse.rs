//! Hand-written Server-Sent Events parser.
//! No crate, because the SSE an OpenAI-compatible server actually uses is three rules and
//! every crate hides the one thing we must control: the split between two socket reads.

use crate::wire::ndjson::LineDecoder;

/// One complete event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field. OpenAI-compatible servers rarely send it, but some do.
    pub name: Option<String>,
    /// The joined `data:` lines.
    pub data: String,
}

impl SseEvent {
    /// OpenAI's end-of-stream marker. Not JSON, so it must be caught before parsing.
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Stateful decoder: eats bytes, yields events.
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

    /// Eat a byte slice and return every complete event.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for line in self.lines.push(bytes) {
            if let Some(event) = self.line(&line) {
                events.push(event);
            }
        }
        events
    }

    /// The partial event when the stream closes without a final blank line; some servers close right after `data: [DONE]`.
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
            // Extra blank line between events: emit nothing, just clear a stray `event:`.
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
