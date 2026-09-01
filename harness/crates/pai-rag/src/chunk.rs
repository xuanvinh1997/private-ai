//! Cắt tài liệu thành đoạn.
//!
//! Hai bất biến, và bài kiểm chứng khoá cả hai.
//!
//! **1. `start`/`end` là offset byte và luôn rơi vào ranh giới ký tự UTF-8.** Đây là cái
//! bẫy chính của module. "Cắt ở ký tự thứ 1000" viết bằng `&text[..1000]` chạy đúng suốt
//! quá trình phát triển bằng tiếng Anh rồi panic ở tệp tiếng Việt đầu tiên, vì `ế` chiếm
//! ba byte và chỉ số 1000 rơi vào giữa nó. Cách chắc chắn duy nhất là **không bao giờ tự
//! tính ra một chỉ số byte**: mọi ranh giới ở đây đến từ `char_indices`, từ `lines`, hoặc
//! từ `trim`, tức là từ những phép luôn trả về ranh giới ký tự.
//!
//! **2. Không mất chữ.** Mọi đoạn là một lát liền mạch `text[start..end]`, và các lát phủ
//! kín mọi ký tự không phải khoảng trắng. Đoạn sau bắt đầu **trước** chỗ đoạn trước kết
//! thúc — đó là phần chồng lấn, và nó tồn tại vì câu trả lời hay nhất thường nằm vắt qua
//! ranh giới: một câu hỏi và một câu đáp ở hai đoạn khác nhau thì không đoạn nào trả lời
//! được nó.
//!
//! Thứ tự ưu tiên khi chọn chỗ cắt: **tiêu đề → đoạn văn → câu → cắt cứng.** Cắt cứng chỉ
//! xảy ra với một câu dài hơn cả một đoạn, tức là một bảng biểu hoặc một khối mã.

/// Tuỳ chọn cắt đoạn. Đơn vị là **ký tự**, không phải byte: một trần tính bằng byte
/// khiến đoạn tiếng Việt ngắn hơn đoạn tiếng Anh khoảng một phần ba, mà cửa sổ ngữ cảnh
/// của mô hình thì đếm token chứ không đếm byte.
#[derive(Clone, Copy, Debug)]
pub struct ChunkOpts {
    /// Kích thước mong muốn của một đoạn.
    pub target: usize,
    /// Bao nhiêu ký tự cuối đoạn trước được lặp lại ở đầu đoạn sau.
    pub overlap: usize,
}

impl Default for ChunkOpts {
    /// ~1000 ký tự, chồng lấn ~150.
    ///
    /// 1000 ký tự là khoảng 250–350 token — đủ dài để một đoạn mang trọn một ý, đủ ngắn
    /// để mười đoạn còn nhét vừa một prompt cùng với lịch sử hội thoại. 150 là khoảng một
    /// hai câu: đủ để một câu bị cắt đôi vẫn nguyên vẹn ở một trong hai đoạn.
    fn default() -> ChunkOpts {
        ChunkOpts {
            target: 1000,
            overlap: 150,
        }
    }
}

impl ChunkOpts {
    pub fn new(target: usize, overlap: usize) -> ChunkOpts {
        // Chồng lấn bằng hoặc lớn hơn đoạn thì đoạn sau chứa trọn đoạn trước và việc cắt
        // không tiến lên được. Siết ở đây thay vì tin người gọi.
        let target = target.max(1);
        ChunkOpts {
            target,
            overlap: overlap.min(target.saturating_sub(1)),
        }
    }
}

/// Một đoạn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub ord: u32,
    pub text: String,
    /// Offset byte trong văn bản gốc, luôn ở ranh giới ký tự.
    pub start: usize,
    pub end: usize,
    /// Tiêu đề mục đang có hiệu lực. Đi vào FTS5 cùng nội dung, vì "phần Bảo mật nói gì"
    /// là một câu hỏi mà chỉ nội dung đoạn không trả lời được.
    pub heading: Option<String>,
}

/// Mức ranh giới mà một đơn vị bắt đầu. Nhỏ hơn là mạnh hơn.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Level {
    Heading,
    Paragraph,
    Sentence,
    Hard,
}

/// Đơn vị nhỏ nhất mà thuật toán chịu tách rời nhau.
struct Unit {
    start: usize,
    end: usize,
    chars: usize,
    level: Level,
    heading: Option<String>,
}

pub fn chunk(text: &str, opts: ChunkOpts) -> Vec<Chunk> {
    let units = units(text, opts.target);
    pack(text, &units, opts)
}

/// Bước một: văn bản → đơn vị, theo đúng thứ tự ưu tiên đã nói ở đầu tệp.
fn units(text: &str, target: usize) -> Vec<Unit> {
    let mut units = Vec::new();
    for block in blocks(text) {
        let chars = text[block.start..block.end].chars().count();
        if chars <= target {
            units.push(Unit {
                start: block.start,
                end: block.end,
                chars,
                level: block.level,
                heading: block.heading,
            });
            continue;
        }
        // Đoạn văn dài hơn cả một chunk: xuống một mức, cắt theo câu.
        for (start, end) in sentences(text, block.start, block.end) {
            let chars = text[start..end].chars().count();
            if chars <= target {
                units.push(Unit {
                    start,
                    end,
                    chars,
                    level: Level::Sentence,
                    heading: block.heading.clone(),
                });
                continue;
            }
            // Và một "câu" dài hơn cả một chunk là một bảng biểu hoặc một khối mã không có
            // dấu chấm nào. Đến đây thì không còn ranh giới ngữ nghĩa nào để tôn trọng.
            for (start, end) in hard_split(text, start, end, target) {
                units.push(Unit {
                    start,
                    end,
                    chars: text[start..end].chars().count(),
                    level: Level::Hard,
                    heading: block.heading.clone(),
                });
            }
        }
    }
    units
}

struct Block {
    start: usize,
    end: usize,
    level: Level,
    heading: Option<String>,
}

/// Khối văn bản: một dòng tiêu đề markdown, hoặc một chuỗi dòng liền nhau giữa hai dòng
/// trắng. Dòng trắng và khoảng trắng thụt đầu dòng không thuộc khối nào — chúng là dấu
/// ngăn, và giữ chúng lại chỉ làm mọi phép đếm ký tự sai đi.
fn blocks(text: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut heading: Option<String> = None;
    let mut open: Option<Block> = None;
    let mut offset = 0usize;

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();

        let trimmed = line.trim();
        // `trim_start`/`trim_end` cắt theo ký tự, nên hai chỉ số này luôn hợp lệ.
        let start = line_start + (line.len() - line.trim_start().len());
        let end = line_start + line.trim_end().len();

        if trimmed.is_empty() {
            if let Some(block) = open.take() {
                blocks.push(block);
            }
            continue;
        }

        if let Some(title) = markdown_heading(trimmed) {
            if let Some(block) = open.take() {
                blocks.push(block);
            }
            heading = Some(title.to_string());
            blocks.push(Block {
                start,
                end,
                level: Level::Heading,
                // Dòng tiêu đề mang chính nó làm tiêu đề: một đoạn bắt đầu bằng nó phải
                // được gán vào mục mới, không phải mục vừa kết thúc.
                heading: heading.clone(),
            });
            continue;
        }

        match &mut open {
            Some(block) => block.end = end,
            None => {
                open = Some(Block {
                    start,
                    end,
                    level: Level::Paragraph,
                    heading: heading.clone(),
                })
            }
        }
    }

    if let Some(block) = open.take() {
        blocks.push(block);
    }
    blocks
}

/// `## Tiêu đề` → `Tiêu đề`. Yêu cầu có khoảng trắng sau dấu thăng, nếu không thì `#hashtag`
/// trong một dòng chữ bình thường sẽ đổi tiêu đề của cả phần còn lại.
fn markdown_heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    let title = rest.trim();
    (!title.is_empty()).then_some(title)
}

/// Cắt một khối thành câu.
///
/// Ranh giới là một dấu kết câu **theo sau bởi khoảng trắng**. Điều kiện "theo sau bởi
/// khoảng trắng" loại đúng hai thứ hay bị cắt nhầm: số thập phân (`3.14`) và tên miền
/// (`example.com`).
fn sentences(text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let slice = &text[start..end];
    let mut out = Vec::new();
    let mut open = start;
    let mut chars = slice.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if !matches!(ch, '.' | '!' | '?' | '…' | ';') {
            continue;
        }
        let next_is_space = chars.peek().is_some_and(|(_, next)| next.is_whitespace());
        if !next_is_space {
            continue;
        }
        let cut = start + offset + ch.len_utf8();
        if let Some(range) = trimmed_range(text, open, cut) {
            out.push(range);
        }
        open = cut;
    }
    if let Some(range) = trimmed_range(text, open, end) {
        out.push(range);
    }
    out
}

/// Cắt cứng, ưu tiên ranh giới từ.
///
/// Trượt về khoảng trắng gần nhất trong một phần tư cuối: cắt giữa một từ làm hỏng cả
/// việc nhúng lẫn việc đọc, còn mất vài chục ký tự thì không.
fn hard_split(text: &str, start: usize, end: usize, target: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut open = start;
    while open < end {
        let slice = &text[open..end];
        let mut cut = end;
        let mut last_space: Option<usize> = None;
        for (count, (offset, ch)) in slice.char_indices().enumerate() {
            if ch.is_whitespace() && count >= target * 3 / 4 {
                last_space = Some(open + offset);
            }
            if count == target {
                cut = last_space.unwrap_or(open + offset);
                break;
            }
        }
        // `cut` bằng `open` chỉ xảy ra nếu `target` bằng 0, mà `ChunkOpts::new` đã chặn.
        // Vẫn giữ lối thoát này: một vòng lặp không tiến là một ứng dụng treo, và giá của
        // việc phòng nó là một dòng.
        if cut <= open {
            cut = end;
        }
        if let Some(range) = trimmed_range(text, open, cut) {
            out.push(range);
        }
        open = cut;
    }
    out
}

/// Bỏ khoảng trắng hai đầu một khoảng byte, hoặc `None` nếu bên trong chỉ có khoảng trắng.
fn trimmed_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let tail = slice.len() - slice.trim_end().len();
    let (start, end) = (start + lead, end - tail);
    (start < end).then_some((start, end))
}

/// Bước hai: gộp đơn vị thành đoạn, rồi lùi lại lấy phần chồng lấn.
fn pack(text: &str, units: &[Unit], opts: ChunkOpts) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    // Byte bắt đầu của đoạn đang mở, khi nó thừa hưởng phần đuôi của đoạn trước.
    let mut carry: Option<usize> = None;
    let mut chars = 0usize;

    // Ngưỡng để một tiêu đề được quyền mở đoạn mới. Không có nó thì một tài liệu toàn
    // tiêu đề ngắn sinh ra mỗi tiêu đề một đoạn; có nó thì các mục ngắn được gom lại.
    let min_fill = opts.target / 3;

    for (index, unit) in units.iter().enumerate() {
        let too_full = chars + unit.chars > opts.target;
        let new_section = unit.level == Level::Heading && chars >= min_fill;
        if !open.is_empty() && (too_full || new_section) {
            let done = assemble(text, units, &open, carry, chunks.len() as u32);
            carry = overlap_start(text, done.start, done.end, opts.overlap);
            chars = carry
                .map(|at| text[at..done.end].chars().count())
                .unwrap_or(0);
            chunks.push(done);
            open.clear();
        }
        open.push(index);
        chars += unit.chars;
    }
    if !open.is_empty() {
        chunks.push(assemble(text, units, &open, carry, chunks.len() as u32));
    }
    chunks
}

/// Đoạn kế bắt đầu từ đâu, để nó chồng lên đuôi đoạn vừa đóng.
///
/// Bản đầu tiên của hàm này mang sang **nguyên những đơn vị cuối** vừa vặn trong ngân
/// sách chồng lấn, và nó im lặng không làm gì cả: một đoạn văn thường dài hai ba trăm ký
/// tự, ngân sách chồng lấn là 150, nên không đơn vị nào vừa và mọi đoạn ra đời không có
/// chồng lấn. Bài kiểm chứng bắt được đúng chỗ đó.
///
/// Nên phép tính ở đây là **theo ký tự, cắt vào giữa một đơn vị** — lùi `overlap` ký tự
/// từ cuối đoạn, rồi trượt tới đầu từ kế tiếp để không mở đoạn bằng nửa một từ. Trả về
/// `None` khi cả đoạn còn ngắn hơn phần chồng lấn: khi ấy đoạn sau sẽ chứa trọn đoạn
/// trước, và việc cắt không tiến lên được.
fn overlap_start(text: &str, start: usize, end: usize, overlap: usize) -> Option<usize> {
    if overlap == 0 || end <= start {
        return None;
    }
    let slice = &text[start..end];
    let total = slice.chars().count();
    if total <= overlap {
        return None;
    }
    // `char_indices` là thứ giữ cho mọi chỉ số ở đây nằm đúng ranh giới ký tự.
    let skip = total - overlap;
    let mut at = start;
    for (count, (offset, _)) in slice.char_indices().enumerate() {
        if count == skip {
            at = start + offset;
            break;
        }
    }
    let tail = &text[at..end];
    let at = match tail.find(char::is_whitespace) {
        Some(cut) => {
            let rest = &tail[cut..];
            at + cut + (rest.len() - rest.trim_start().len())
        }
        // Một đuôi không có khoảng trắng nào là một từ dài hơn cả phần chồng lấn; cắt
        // giữa nó vẫn hơn là bỏ hẳn chồng lấn.
        None => at,
    };
    (at > start && at < end).then_some(at)
}

fn assemble(text: &str, units: &[Unit], open: &[usize], carry: Option<usize>, ord: u32) -> Chunk {
    // `open` luôn không rỗng ở mọi chỗ gọi; ba `unwrap_or` dưới đây chỉ để không có
    // `unwrap` nào trên đường chạy thật.
    let first = open.first().map(|i| units[*i].start).unwrap_or(0);
    // Phần thừa hưởng luôn nằm trước đơn vị đầu tiên — nó là đuôi của đoạn trước, mà các
    // đơn vị thì không chồng lên nhau. `min` chỉ để bất biến đó không phụ thuộc vào việc
    // đọc đúng chỗ gọi.
    let start = carry.map(|at| at.min(first)).unwrap_or(first);
    let end = open.last().map(|i| units[*i].end).unwrap_or(0);
    Chunk {
        ord,
        text: text[start..end].to_string(),
        start,
        end,
        // Tiêu đề lấy theo **đơn vị đầu tiên**, không theo phần thừa hưởng: đoạn này
        // thuộc về mục mà nội dung mới của nó nằm trong, không thuộc mục vừa kết thúc.
        heading: open.first().and_then(|i| units[*i].heading.clone()),
    }
}
