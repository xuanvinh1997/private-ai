//! Bộ đệm đầu ra có trần.
//!
//! Một máy chủ phát triển in ra hàng giờ, và không ai đọc phần giữa. Ba quyết định:
//!
//! **Giữ phần mới nhất.** Dòng cuối là dòng nói vì sao mọi thứ dừng lại; dòng đầu là dòng
//! nói phiên bản của một thư viện.
//!
//! **Nói ra phần đã bỏ.** Cắt trong im lặng để mô hình kết luận trên một bản ghi thiếu mà
//! không biết là thiếu — nó sẽ đọc "không có lỗi nào" từ một khoảng trống. Nên [`Page`]
//! mang theo số dòng đã rơi, và tool in nó ra thành chữ.
//!
//! **`\r` lẻ ghi đè thay vì xuống dòng.** Một thanh tiến trình vẽ lại chính nó hàng nghìn
//! lần bằng carriage return. Coi mỗi lần vẽ lại là một dòng mới thì cái trần bị một thanh
//! tiến trình duy nhất ăn hết, và thứ bị đẩy ra ngoài là mọi thứ đáng đọc.
//!
//! Nhưng `\r\n` thì là **một** lần xuống dòng, không phải một lần xoá rồi một lần xuống
//! dòng. Đây không phải chuyện lý thuyết: một PTY ở chế độ cooked bật `ONLCR`, nên mọi
//! `\n` mà chương trình in ra tới đây thành `\r\n`. Xử lý `\r` trước rồi mới nhìn `\n` sẽ
//! xoá sạch từng dòng ngay trước khi ghi nó xuống — và cái hỏng ra là một bộ đệm đầy dòng
//! rỗng, đúng số dòng, đúng thứ tự, không một chữ nào.
//!
//! Đây là chỗ duy nhất trong crate có mùi mô phỏng terminal, và nó dừng đúng ở đây: mô
//! phỏng đủ để cái trần có nghĩa, không đi xa hơn.

use std::collections::VecDeque;

/// Trần cho một dòng chưa kết thúc, tính bằng byte.
///
/// Một chương trình in ra hàng megabyte không có `\n` nào không được phép biến bộ đệm
/// "có trần" thành một chuỗi lớn vô hạn.
const MAX_PENDING: usize = 64 * 1024;

/// Một trang đọc ra từ bộ đệm.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub lines: Vec<String>,
    /// Tổng số dòng đã bị bỏ vì vượt trần, tính từ lúc mở phiên.
    pub dropped: usize,
    /// Số dòng đang có trong bộ đệm.
    pub retained: usize,
}

/// Vòng đệm theo dòng.
pub struct Ring {
    lines: VecDeque<String>,
    pending: String,
    /// Đã thấy `\r` và chưa biết ký tự sau nó là gì. Xem [`Ring::push`].
    after_cr: bool,
    dropped: usize,
    /// Tổng số dòng từng được ghi vào, kể cả những dòng đã rơi ra ngoài. Đây là cái đồng
    /// hồ mà [`crate::provider`] dùng để trả lời "có gì mới kể từ lúc tôi gửi lệnh".
    produced: u64,
    max_lines: usize,
}

impl Ring {
    pub fn new(max_lines: usize) -> Ring {
        Ring {
            lines: VecDeque::new(),
            pending: String::new(),
            after_cr: false,
            dropped: 0,
            produced: 0,
            // Một trần bằng 0 biến mọi lần ghi thành một lần bỏ, và cái đó thì không ai
            // muốn kể cả khi đã gõ ra.
            max_lines: max_lines.max(1),
        }
    }

    /// Số dòng đã hoàn tất từ trước tới giờ. Đơn điệu tăng.
    pub fn produced(&self) -> u64 {
        self.produced
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Nuốt một mẩu byte từ PTY.
    ///
    /// Giải mã lossy chứ không từ chối: một mẩu có thể cắt ngang một ký tự nhiều byte, và
    /// vứt cả mẩu vì một ký tự dở là mất một dòng lỗi để đổi lấy sự chính xác về mã hoá.
    pub fn push(&mut self, chunk: &[u8]) {
        for ch in String::from_utf8_lossy(chunk).chars() {
            match ch {
                // Quyết định hoãn lại tới ký tự sau: chỉ lúc đó mới biết đây là `\r` lẻ
                // hay là nửa đầu của `\r\n`. Cờ sống qua các lần `push` vì một mẩu byte
                // được phép kết thúc đúng giữa hai nửa ấy.
                '\r' => self.after_cr = true,
                '\n' => {
                    self.after_cr = false;
                    let line = std::mem::take(&mut self.pending);
                    self.commit(line);
                }
                _ => {
                    if std::mem::take(&mut self.after_cr) {
                        self.pending.clear();
                    }
                    self.pending.push(ch);
                    if self.pending.len() >= MAX_PENDING {
                        let line = std::mem::take(&mut self.pending);
                        self.commit(line);
                    }
                }
            }
        }
    }

    fn commit(&mut self, line: String) {
        self.lines.push_back(line);
        self.produced += 1;
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }

    /// Trang tính từ dòng mới nhất về sau. `offset = 0` là trang mới nhất.
    ///
    /// Dòng dở dang cũng được trả về, vì lời nhắc của shell và câu hỏi "y/n" của một tiện
    /// ích đều là những dòng không bao giờ có `\n` — và đó chính là những dòng mà người
    /// đọc cần thấy nhất.
    pub fn page(&self, offset: usize, count: usize) -> Page {
        let mut all: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        if !self.pending.is_empty() {
            all.push(&self.pending);
        }
        let end = all.len().saturating_sub(offset);
        let start = end.saturating_sub(count);
        Page {
            lines: all[start..end].iter().map(|s| s.to_string()).collect(),
            dropped: self.dropped,
            retained: all.len(),
        }
    }

    /// Mọi dòng hoàn tất kể từ mốc `since` (một giá trị [`Ring::produced`] đã lấy trước đó).
    ///
    /// Mốc nằm trong phần đã rơi thì trả về những gì còn giữ được: một câu trả lời thiếu
    /// nhưng nói được là thiếu vẫn hơn một câu trả lời trống.
    pub fn since(&self, since: u64) -> Vec<String> {
        let fresh = self.produced.saturating_sub(since) as usize;
        let take = fresh.min(self.lines.len());
        let mut out: Vec<String> = self
            .lines
            .iter()
            .skip(self.lines.len() - take)
            .cloned()
            .collect();
        if !self.pending.is_empty() {
            out.push(self.pending.clone());
        }
        out
    }

    /// Quên hết. Dùng đúng một lần, sau khi phiên đã được mồi xong — xem
    /// [`crate::session`].
    pub fn reset(&mut self) {
        self.lines.clear();
        self.pending.clear();
        self.after_cr = false;
    }
}

#[cfg(test)]
mod tests {
    use super::Ring;

    #[test]
    fn tran_giu_phan_moi_va_dem_phan_bo() {
        let mut ring = Ring::new(3);
        ring.push(b"mot\nhai\nba\nbon\nnam\n");
        let page = ring.page(0, 10);
        assert_eq!(page.lines, vec!["ba", "bon", "nam"]);
        assert_eq!(page.dropped, 2);
    }

    #[test]
    fn carriage_return_le_ve_lai_dong_hien_tai() {
        let mut ring = Ring::new(10);
        ring.push(b"10%\r50%\r100%\nxong\n");
        assert_eq!(ring.page(0, 10).lines, vec!["100%", "xong"]);
    }

    /// PTY ở chế độ cooked bật `ONLCR`, nên đây là hình dạng thật của mọi dòng đi qua đây.
    #[test]
    fn crlf_la_mot_lan_xuong_dong_chu_khong_phai_mot_lan_xoa() {
        let mut ring = Ring::new(10);
        ring.push(b"mot\r\nhai\r\n");
        assert_eq!(ring.page(0, 10).lines, vec!["mot", "hai"]);
    }

    /// Một mẩu byte được phép kết thúc đúng giữa `\r` và `\n`.
    #[test]
    fn crlf_bi_cat_giua_hai_mau_van_la_mot_dong() {
        let mut ring = Ring::new(10);
        ring.push(b"mot\r");
        ring.push(b"\nhai\r\n");
        assert_eq!(ring.page(0, 10).lines, vec!["mot", "hai"]);
    }

    #[test]
    fn offset_dem_tu_dong_moi_nhat() {
        let mut ring = Ring::new(10);
        ring.push(b"a\nb\nc\nd\n");
        assert_eq!(ring.page(1, 2).lines, vec!["b", "c"]);
    }
}
