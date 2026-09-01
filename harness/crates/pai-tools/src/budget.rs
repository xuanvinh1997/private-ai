//! Ngân sách của một kết quả tool, đo bằng token chứ không bằng dòng.
//!
//! **Vì sao không đếm dòng.** Trần theo số dòng không tương quan với thứ thật sự cạn:
//! cửa sổ ngữ cảnh. Một tệp JSON tối giản 100 dòng vượt ngân sách trong khi một tệp Rust
//! 2000 dòng thưa thì không, nên "256 dòng" vừa cắt nhầm cái ngắn vừa thả lọt cái dài.
//! Đếm dòng là đếm nhầm thứ. Ở đây đơn vị là **byte chia bốn** — xấp xỉ token, sai số
//! vài chục phần trăm nhưng sai cùng chiều với thứ ta muốn giữ.
//!
//! **Vì sao đầu *và* đuôi.** Chỗ mô hình cần gần như luôn nằm ở một trong hai đầu: phần
//! đầu nói tệp này là cái gì, phần đuôi nói nó kết thúc ra sao (mã thoát, dòng lỗi cuối,
//! khớp cuối cùng). Giữ mỗi phần đầu là dạy mô hình rằng một lệnh chạy 10 nghìn dòng
//! không có mã thoát.
//!
//! **Vì sao phải nói ra cách lấy tiếp.** Một dòng "…(đã cắt)" không kèm cách lấy phần dư
//! dạy mô hình kết luận rằng nó đã thấy hết. Đó là kiểu nói dối tệ nhất một kết quả tool
//! có thể làm, nên [`Overflow::fold`] **bắt buộc** người gọi cung cấp câu chỉ dẫn — nó là
//! tham số, không phải tuỳ chọn.

use pai_core::Context;

use crate::name::ToolName;
use crate::seam::Spill;
use crate::spill::SpillRef;

/// Bao nhiêu byte thì xấp xỉ một token.
///
/// Bốn là con số các bộ tách token BPE cho văn bản Latin thường rơi vào. Tiếng Việt có
/// dấu tốn nhiều byte hơn mỗi ký tự, nên phép xấp xỉ này *đánh giá cao* số token của văn
/// bản tiếng Việt — sai về phía dè dặt, đúng phía cần sai.
pub const BYTES_PER_TOKEN: usize = 4;

/// Ngân sách mặc định cho một kết quả tool, tính bằng token xấp xỉ (~24 KiB).
///
/// Codex dừng ở 10 KiB, và cộng đồng của nó báo rằng con số đó phá vỡ việc đọc trọn một
/// tệp bình thường. Hai mươi tư KiB đủ cho hầu hết tệp mã nguồn thật mà vẫn còn chỗ cho
/// vài lượt nữa trong một cửa sổ 200k.
pub const DEFAULT_TOKEN_BUDGET: usize = 6_000;

/// Số token xấp xỉ của một đoạn văn bản.
pub fn approx_tokens(text: &str) -> usize {
    text.len().div_ceil(BYTES_PER_TOKEN)
}

/// Một kết quả đã bị gấp lại: phần đầu, phần đuôi, và những gì nằm giữa.
#[derive(Debug, PartialEq)]
pub struct Split<'a> {
    pub head: &'a str,
    pub tail: &'a str,
    /// Số dòng **trọn vẹn** trong phần đầu. Đây là con số để tính `offset` cho lần gọi
    /// tiếp: một dòng bị cắt giữa chừng không được tính là đã đọc.
    pub head_lines: usize,
    pub tail_lines: usize,
    pub total_lines: usize,
    pub omitted_lines: usize,
    pub total_bytes: usize,
}

/// Kết quả sau khi áp ngân sách.
pub struct Folded {
    /// Văn bản mô hình đọc — đã kèm lời chỉ dẫn lấy tiếp nếu có cắt.
    pub content: String,
    /// Vé lấy lại toàn văn. `None` khi không cắt, hoặc khi không có kho nào cắm vào.
    pub spill: Option<SpillRef>,
    pub truncated: bool,
    pub omitted_lines: usize,
    pub total_lines: usize,
}

/// Chỗ một tool áp ngân sách lên kết quả của chính nó.
///
/// Giữ `Context` chứ không giữ sẵn kho tràn: kho được hỏi **tại thời điểm gọi**, giống
/// mọi seam khác trong crate này. Gỡ kho ra phải làm mọi lần cắt sau đó biết là mình
/// không còn chỗ cất, chứ không phải đi qua một bản sao còn sót lại.
#[derive(Clone)]
pub struct Overflow {
    ctx: Context,
    budget: usize,
}

impl Overflow {
    pub fn new(ctx: &Context) -> Overflow {
        Overflow {
            ctx: ctx.clone(),
            budget: DEFAULT_TOKEN_BUDGET,
        }
    }

    /// Ngân sách tính bằng token xấp xỉ.
    pub fn with_budget(mut self, tokens: usize) -> Overflow {
        self.budget = tokens.max(1);
        self
    }

    pub fn budget_tokens(&self) -> usize {
        self.budget
    }

    fn budget_bytes(&self) -> usize {
        self.budget.saturating_mul(BYTES_PER_TOKEN)
    }

    /// Vừa ngân sách thì `None`; không vừa thì đầu và đuôi, mỗi bên nửa ngân sách.
    pub fn split<'a>(&self, full: &'a str) -> Option<Split<'a>> {
        let budget = self.budget_bytes();
        if full.len() <= budget {
            return None;
        }
        let half = (budget / 2).max(1);
        let head = head_slice(full, half);
        let tail = tail_slice(full, half);
        let total_lines = count_lines(full);
        let head_lines = head.matches('\n').count();
        let tail_lines = count_lines(tail);
        Some(Split {
            head,
            tail,
            head_lines,
            tail_lines,
            total_lines,
            omitted_lines: total_lines.saturating_sub(head_lines + tail_lines),
            total_bytes: full.len(),
        })
    }

    /// Cất toàn văn vào kho. `None` nghĩa là chưa ai cắm kho vào cây.
    pub fn store(&self, tool: &ToolName, full: &str) -> Option<SpillRef> {
        self.ctx.get::<Spill>().map(|store| store.spill(tool, full))
    }

    /// Gấp một kết quả cho vừa ngân sách.
    ///
    /// `resume` nhận chỗ cắt và trả về **câu chỉ dẫn lấy tiếp** của chính tool đó — với
    /// `read` là một `offset` cụ thể, với `grep` là một mẫu hẹp hơn. Nó là tham số bắt
    /// buộc vì cắt mà không nói cách lấy tiếp là dạy mô hình kết luận nó đã thấy hết.
    ///
    /// Không có kho tràn nào cắm vào thì **không cắt gì cả**: dài thì còn sửa được, mất
    /// thì không, và một lời chỉ dẫn trỏ tới một cái kho không tồn tại là một lời hứa suông.
    pub fn fold(
        &self,
        tool: &ToolName,
        full: String,
        resume: impl FnOnce(&Split<'_>) -> String,
    ) -> Folded {
        let Some(split) = self.split(&full) else {
            return Folded {
                truncated: false,
                total_lines: count_lines(&full),
                content: full,
                spill: None,
                omitted_lines: 0,
            };
        };
        let Some(handle) = self.store(tool, &full) else {
            tracing::warn!(tool = %tool, "vượt ngân sách nhưng chưa có kho tràn nào cắm vào");
            return Folded {
                truncated: false,
                total_lines: split.total_lines,
                content: full,
                spill: None,
                omitted_lines: 0,
            };
        };

        let hint = resume(&split);
        // Nói cả byte lẫn dòng: một tệp một dòng dài 200 KiB bị cắt mà "0 dòng bị bỏ" là
        // một con số đúng nhưng đọc thành một lời trấn an sai.
        let omitted_bytes = split
            .total_bytes
            .saturating_sub(split.head.len())
            .saturating_sub(split.tail.len());
        let content = format!(
            "{}\n[… đã cắt bớt {omitted_bytes} byte ở giữa (~{} token, {} dòng trọn vẹn) \
             cho vừa ngân sách {} token. Toàn văn {} dòng vẫn nguyên vẹn trong kho: gọi \
             `spill_read` với `id: \"{}\"`. {hint}]\n{}",
            split.head.trim_end_matches('\n'),
            omitted_bytes.div_ceil(BYTES_PER_TOKEN),
            split.omitted_lines,
            self.budget,
            handle.lines,
            handle.id,
            split.tail.trim_start_matches('\n'),
        );

        Folded {
            content,
            truncated: true,
            omitted_lines: split.omitted_lines,
            total_lines: split.total_lines,
            spill: Some(handle),
        }
    }
}

/// Số dòng, đếm cả dòng cuối không có `\n`.
fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().count()
}

/// Lùi về ranh giới ký tự gần nhất không vượt `idx`.
fn floor_boundary(text: &str, mut idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Tiến tới ranh giới ký tự gần nhất không nhỏ hơn `idx`.
fn ceil_boundary(text: &str, mut idx: usize) -> usize {
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx.min(text.len())
}

/// Phần đầu, cắt ở cuối dòng nếu việc đó không vứt đi quá nửa phần được cấp.
///
/// Điều kiện "quá nửa" là chỗ xử lý tệp ít dòng mà dòng rất dài: một dòng JSON 200 KiB
/// không có `\n` nào ở trong tầm, và lúc đó cắt giữa dòng vẫn tốt hơn trả về rỗng.
fn head_slice(text: &str, budget: usize) -> &str {
    let mut end = floor_boundary(text, budget);
    if let Some(newline) = text[..end].rfind('\n')
        && newline + 1 >= budget / 2
    {
        end = newline + 1;
    }
    &text[..end]
}

/// Phần đuôi, theo cùng luật nhưng soi từ phía sau.
fn tail_slice(text: &str, budget: usize) -> &str {
    let start = ceil_boundary(text, text.len().saturating_sub(budget));
    let rest = &text[start..];
    if let Some(newline) = rest.find('\n')
        && newline <= budget / 2
    {
        return &rest[newline + 1..];
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(tokens: usize) -> Overflow {
        Overflow::new(&Context::root()).with_budget(tokens)
    }

    #[test]
    fn vua_ngan_sach_thi_khong_cat() {
        assert!(bench(100).split("ngắn").is_none());
    }

    /// Bài chứng minh đếm dòng là sai: ba dòng, nhưng mỗi dòng rất dài.
    #[test]
    fn it_dong_ma_dong_rat_dai_van_bi_cat() {
        let full = format!(
            "{}\n{}\n{}\n",
            "a".repeat(4000),
            "b".repeat(4000),
            "c".repeat(4000)
        );
        let split = bench(100).split(&full).expect("vượt 400 byte thì phải cắt");
        assert!(split.head.starts_with('a'));
        assert!(split.tail.ends_with('c') || split.tail.ends_with("c\n"));
        assert!(split.head.len() + split.tail.len() < full.len());
    }

    /// Một dòng duy nhất, không có `\n` nào để mà cắt cho gọn.
    #[test]
    fn mot_dong_duy_nhat_van_cat_duoc_giua_dong() {
        let full = "x".repeat(10_000);
        let split = bench(100).split(&full).expect("phải cắt");
        assert!(!split.head.is_empty(), "cắt giữa dòng còn hơn trả về rỗng");
        assert!(!split.tail.is_empty());
    }

    #[test]
    fn cat_khong_bao_gio_roi_vao_giua_mot_ky_tu() {
        let full = "đường dẫn tiếng Việt ".repeat(500);
        let split = bench(50).split(&full).expect("phải cắt");
        // `&str` chỉ dựng được ở ranh giới ký tự; nếu sai thì bài này đã panic ở trên.
        assert!(full.starts_with(split.head));
        assert!(full.ends_with(split.tail));
    }
}
