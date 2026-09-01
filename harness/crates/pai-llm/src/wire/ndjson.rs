//! Cắt một dòng byte thành từng dòng văn bản.
//!
//! Ollama trả NDJSON ở cả `/api/chat` lẫn `/api/pull` — mỗi dòng một object JSON, không
//! có `data:`, không có dòng trống ngăn cách. Đơn giản hơn SSE, nhưng vẫn dính đúng cái
//! bẫy ấy: một lần đọc socket có thể dừng ở giữa dòng.

/// Bộ đệm byte cắt theo `\n`.
#[derive(Debug, Default)]
pub struct LineDecoder {
    buffer: Vec<u8>,
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ăn một mảnh byte, trả về mọi dòng **đã trọn vẹn**. Phần đuôi dở dang ở lại trong
    /// bộ đệm chờ lần đọc sau.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        // `drain` từ đầu mỗi lần tìm thấy `\n`: bộ đệm không bao giờ lớn hơn một dòng dở.
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let raw: Vec<u8> = self.buffer.drain(..=position).collect();
            lines.push(decode(&raw[..position]));
        }
        lines
    }

    /// Phần còn lại khi luồng đóng mà không có `\n` cuối. Một số máy chủ làm vậy ở dòng
    /// chót, nên bỏ nó đi là mất đúng cái dòng `done: true`.
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

/// Giải mã một dòng đã trọn vẹn, bỏ `\r` của CRLF.
///
/// `from_utf8_lossy` chứ không `from_utf8(...).expect(...)`: một dòng lỗi mã hoá phải làm
/// hỏng đúng dòng đó, không được giết cả lượt trả lời.
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
