//! One ceiling, applied in one place.
//!
//! A 2 MB article and a 5000-row JSON array both arrive as "some text", and neither may be
//! allowed to swallow the context window. The notice is produced here rather than by the caller
//! on purpose: a truncation the model is not told about is a truncation the model will reason
//! wrongly about, and a rule every caller has to remember is a rule that gets forgotten.

/// How far back a cut may hunt for a clean boundary, as a fraction of the budget. Beyond this the
/// hunt costs more content than the tidiness is worth, so the cut lands mid-paragraph instead.
const BOUNDARY_REACH: usize = 5;

/// Text after the ceiling was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trimmed {
    /// The text, with the truncation notice already appended when there was one.
    pub text: String,
    pub truncated: bool,
    /// Characters kept, not counting the notice.
    pub kept: usize,
    /// Characters the input had.
    pub total: usize,
}

/// Cut `text` to at most `max_chars` characters, preferring a paragraph break, then a line break,
/// then a word break, and only then an arbitrary character.
///
/// Counted in characters rather than bytes because the budget exists to bound what the model reads,
/// and a Vietnamese page is roughly twice as many bytes as characters.
pub fn trim_to(text: &str, max_chars: usize) -> Trimmed {
    let total = text.chars().count();
    if total <= max_chars {
        return Trimmed {
            text: text.to_string(),
            truncated: false,
            kept: total,
            total,
        };
    }

    // Byte offset of the character just past the budget; always a char boundary, so slicing is safe.
    let ceiling = text
        .char_indices()
        .nth(max_chars)
        .map(|(at, _)| at)
        .unwrap_or(text.len());
    let head = &text[..ceiling];
    let floor = head.len() - head.len() / BOUNDARY_REACH;

    let cut = head
        .rfind("\n\n")
        .filter(|at| *at >= floor)
        .or_else(|| head.rfind('\n').filter(|at| *at >= floor))
        .or_else(|| head.rfind(char::is_whitespace).filter(|at| *at >= floor))
        .unwrap_or(head.len());

    let kept_text = head[..cut].trim_end();
    let kept = kept_text.chars().count();
    let mut out = String::with_capacity(kept_text.len() + 96);
    out.push_str(kept_text);
    out.push_str(&notice(kept, total));
    Trimmed {
        text: out,
        truncated: true,
        kept,
        total,
    }
}

/// Cut a short field -- a title, a snippet, one cell of a list -- to `max_chars`, marking the cut
/// with an ellipsis.
///
/// The sibling of [`trim_to`], and the split is deliberate rather than a shortcut: both say a cut
/// happened, but [`trim_to`]'s notice is a whole paragraph, which is right below a truncated
/// document and absurd inside a heading, where it would be several times longer than the field it
/// explains. What must never happen is a field with no ceiling at all: a title and a URL come from
/// the same stranger the body does, and a caller that bounds only the body has bounded nothing.
pub fn clip(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if out.chars().count() < text.chars().count() {
        out = out.trim_end().to_string();
        out.push('…');
    }
    out
}

/// The sentence the model reads where the content stops.
fn notice(kept: usize, total: usize) -> String {
    format!("\n\n---\n(Đã cắt bớt: chỉ giữ {kept}/{total} ký tự đầu tiên. Phần còn lại chưa được đọc.)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ngan_hon_tran_thi_khong_dong_gi() {
        let trimmed = trim_to("ngắn gọn", 100);
        assert!(!trimmed.truncated);
        assert_eq!(trimmed.text, "ngắn gọn");
        assert_eq!(trimmed.kept, trimmed.total);
    }

    #[test]
    fn cat_o_ranh_gioi_doan_va_noi_ro_da_cat() {
        let body = format!("{}\n\n{}", "a".repeat(80), "b".repeat(80));
        let trimmed = trim_to(&body, 100);
        assert!(trimmed.truncated);
        assert_eq!(trimmed.kept, 80, "phải dừng ở ranh giới đoạn, không cắt giữa đoạn");
        assert!(trimmed.text.starts_with(&"a".repeat(80)));
        assert!(!trimmed.text.contains("bb"), "không được lọt sang đoạn sau");
        assert!(trimmed.text.contains("Đã cắt bớt"));
        assert!(trimmed.text.contains("162"), "phải nói tổng số ký tự gốc");
    }

    #[test]
    fn khong_co_ranh_gioi_thi_van_cat_dung_tran() {
        let body = "x".repeat(500);
        let trimmed = trim_to(&body, 100);
        assert!(trimmed.truncated);
        assert_eq!(trimmed.kept, 100);
    }

    #[test]
    fn clip_ngan_hon_tran_thi_giu_nguyen() {
        assert_eq!(clip("Tiêu đề ngắn", 100), "Tiêu đề ngắn");
    }

    #[test]
    fn clip_cat_va_danh_dau_bang_dau_ba_cham() {
        let clipped = clip(&"chào ".repeat(100), 10);
        assert_eq!(clipped.chars().count(), 10, "phải gồm 9 ký tự nội dung và dấu cắt");
        assert!(clipped.ends_with('…'), "{clipped}");
        // The paragraph-sized notice belongs to `trim_to`; a title has no room for it.
        assert!(!clipped.contains("Đã cắt bớt"), "{clipped}");
    }

    #[test]
    fn khong_cat_giua_mot_ky_tu_nhieu_byte() {
        // Every character here is 3 bytes; a byte-wise cut would produce invalid UTF-8.
        let body = "chào".repeat(200);
        let trimmed = trim_to(&body, 50);
        assert!(trimmed.truncated);
        assert!(trimmed.text.chars().count() > 0);
    }
}
