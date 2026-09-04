//! Capped output buffer: keep the newest lines and report how many were dropped.
//! A lone `\r` redraws the current line so one progress bar cannot eat the cap, while `\r\n`
//! stays a single newline -- a cooked PTY with `ONLCR` turns every `\n` into `\r\n`.

use std::collections::VecDeque;

/// Byte cap for an unterminated line, so a program printing megabytes without `\n` cannot grow the buffer without bound.
const MAX_PENDING: usize = 64 * 1024;

/// One page read out of the buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub lines: Vec<String>,
    /// Total lines dropped for exceeding the cap since the session opened.
    pub dropped: usize,
    /// Lines currently held in the buffer.
    pub retained: usize,
}

/// Line-oriented ring buffer.
pub struct Ring {
    lines: VecDeque<String>,
    pending: String,
    /// Saw a `\r` and does not yet know the next char. See [`Ring::push`].
    after_cr: bool,
    dropped: usize,
    /// Every line ever committed, dropped ones included; the clock [`crate::provider`] uses to answer "what is new".
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
            // A cap of 0 would turn every write into a drop, which nobody wants even if they typed it.
            max_lines: max_lines.max(1),
        }
    }

    /// Completed lines so far. Monotonically increasing.
    pub fn produced(&self) -> u64 {
        self.produced
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Swallow a byte chunk from the PTY. Lossy decoding, since a chunk may split a multi-byte char.
    pub fn push(&mut self, chunk: &[u8]) {
        for ch in String::from_utf8_lossy(chunk).chars() {
            match ch {
                // Defer the decision to the next char: only then is a lone `\r` distinguishable from `\r\n`.
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

    /// A page counting back from the newest line; `offset = 0` is the newest. The pending line is included,
    /// because shell prompts and "y/n" questions never end in `\n`.
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

    /// Completed lines since the `since` mark (an earlier [`Ring::produced`]); if the mark has been dropped, returns what is left.
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

    /// Forget everything. Used exactly once, after the session finishes priming -- see [`crate::session`].
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

    /// A cooked PTY sets `ONLCR`, so this is the real shape of every line arriving here.
    #[test]
    fn crlf_la_mot_lan_xuong_dong_chu_khong_phai_mot_lan_xoa() {
        let mut ring = Ring::new(10);
        ring.push(b"mot\r\nhai\r\n");
        assert_eq!(ring.page(0, 10).lines, vec!["mot", "hai"]);
    }

    /// A byte chunk may end exactly between `\r` and `\n`.
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
