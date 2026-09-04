//! Splits a byte stream into text lines.
//! Ollama returns NDJSON on `/api/chat` and `/api/pull`: one JSON object per line, no
//! `data:` prefix, no blank separator - but a socket read can still stop mid-line.

/// Byte buffer split on `\n`.
#[derive(Debug, Default)]
pub struct LineDecoder {
    buffer: Vec<u8>,
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Eat a byte slice and return every *complete* line; a partial tail waits for the next read.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        // `drain` from the front on every `\n`, so the buffer never exceeds one partial line.
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let raw: Vec<u8> = self.buffer.drain(..=position).collect();
            lines.push(decode(&raw[..position]));
        }
        lines
    }

    /// The remainder when the stream closes without a trailing `\n`; dropping it would lose the `done: true` line.
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        let raw: Vec<u8> = self.buffer.drain(..).collect();
        Some(decode(&raw))
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Decode one complete line, dropping the CRLF `\r`; lossy on purpose, so a bad byte breaks one line rather than the whole turn.
fn decode(raw: &[u8]) -> String {
    let trimmed = raw.strip_suffix(b"\r").unwrap_or(raw);
    String::from_utf8_lossy(trimmed).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dong_dang_do_o_lai_trong_dem() {
        let mut decoder = LineDecoder::new();
        assert_eq!(decoder.push(b"{\"a\":1}\n{\"b\""), vec!["{\"a\":1}"]);
        assert!(!decoder.is_empty());
        assert_eq!(decoder.push(b":2}\n"), vec!["{\"b\":2}"]);
        assert!(decoder.is_empty());
    }

    #[test]
    fn crlf_bi_cat_bo() {
        let mut decoder = LineDecoder::new();
        assert_eq!(decoder.push(b"mot\r\nhai\n"), vec!["mot", "hai"]);
    }

    #[test]
    fn flush_tra_ve_dong_chot_khong_co_newline() {
        let mut decoder = LineDecoder::new();
        assert!(decoder.push(b"cuoi").is_empty());
        assert_eq!(decoder.flush().as_deref(), Some("cuoi"));
        assert_eq!(decoder.flush(), None);
    }
}
