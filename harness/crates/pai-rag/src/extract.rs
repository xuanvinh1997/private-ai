//! Rút chữ khỏi tệp.
//!
//! Một bất biến duy nhất, và nó là lý do cả module tồn tại: **một tệp hỏng chỉ được làm
//! hỏng chính nó.** Người dùng kéo hai mươi tệp vào một lúc; tệp thứ bảy là một PDF bị
//! cắt cụt lúc tải về. Nếu tệp đó giết tiến trình thì mười ba tệp còn lại không bao giờ
//! được nạp, và người dùng không có cách nào biết tệp nào là thủ phạm.
//!
//! Vì thế mọi đường ở đây trả `Result`, và đường PDF còn thêm một lớp
//! [`std::panic::catch_unwind`]: `pdf-extract` **panic** trên cấu trúc dị dạng thay vì
//! trả lỗi, nên không bắt hoảng loạn thì lớp `Result` chỉ là trang trí.
//!
//! Định dạng suy từ phần mở rộng, không phải từ nội dung. Đoán theo magic bytes nghe
//! chắc chắn hơn nhưng sai ở đúng chỗ quan trọng: một tệp `.md` viết bằng chữ Việt trông
//! y hệt một tệp `.txt`, và người dùng đã nói ra ý định của họ ngay trong cái tên.

use std::io::{Cursor, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use crate::error::RagError;

/// Bộ rút chữ đang chạy là bản mấy.
///
/// Thư viện quét tăng dần: tệp có `mtime` và kích thước không đổi thì **không** đi qua bộ
/// rút chữ lần nữa. Bất biến đó tiết kiệm cả một buổi chờ, và nó cũng có nghĩa là một tệp
/// từng đọc ra rác sẽ ở lại dạng rác vĩnh viễn — người dùng không sửa tệp, nên `mtime`
/// không đổi, nên không có gì mời thư viện thử lại.
///
/// Tăng số này mỗi khi bộ rút chữ **đọc ra kết quả khác** trên cùng một tệp. Lần mở kho
/// ngay sau đó sẽ vô hiệu hoá dấu vân tay cũ và lần quét kế tiếp đọc lại cả thư mục — xem
/// [`crate::library::Library::open`]. Đây là bước không có thì một bản vá ở tầng này chỉ
/// tới được với thư viện mới, còn thư viện của người đã dùng phần mềm thì không.
///
/// - **1** — bản đầu.
/// - **2** — vá `adobe-cmap-parser` (xem `vendor/adobe-cmap-parser/README.md`): mọi PDF
///   có tên CMap chứa dấu gạch ngang — Calibre, LibreOffice — trước đó rút ra toàn khoảng
///   trắng và cho 0 đoạn.
pub const EXTRACT_VERSION: u32 = 2;

/// Trần kích thước một tệp được nạp.
///
/// 64 MiB không phải một con số thiêng liêng; nó là chỗ mà cả `pdf-extract` lẫn phần cắt
/// đoạn còn nằm gọn trong RAM của một máy tính cá nhân. Cả hai đều **đọc cả tệp vào bộ
/// nhớ**, nên trần này là thứ đứng giữa một tệp 4 GB và một lần OOM giết cả ứng dụng.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Bao nhiêu byte đầu được soi để kết luận "đây là tệp nhị phân".
///
/// 4096 và chỉ dò byte NUL — cùng ngưỡng, cùng phép thử với `pai_fs::path::looks_binary`.
/// Chép lại thay vì phụ thuộc `pai-fs` là có chủ ý: thư viện tài liệu không cần và không
/// nên với tới seam hệ tệp của agent. Nhưng hai chỗ phải **giống nhau**, vì một tệp bị
/// `read` từ chối mà thư viện lại nhận vào là hai câu trả lời trái nhau cho cùng một tệp.
const BINARY_PROBE: usize = 4096;

/// Định dạng của một tài liệu. Chuỗi serde khớp `DocumentView.format` phía `app/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Pdf,
    Docx,
    Markdown,
    Text,
    Html,
    Csv,
    Code,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Markdown => "markdown",
            Format::Text => "text",
            Format::Html => "html",
            Format::Csv => "csv",
            Format::Code => "code",
        }
    }

    /// Nhãn lạ trong cơ sở dữ liệu chỉ có thể đến từ một bản cũ hơn của chính crate này;
    /// xếp nó vào `text` đọc được hơn là làm hỏng cả câu truy vấn.
    pub fn parse(name: &str) -> Format {
        match name {
            "pdf" => Format::Pdf,
            "docx" => Format::Docx,
            "markdown" => Format::Markdown,
            "html" => Format::Html,
            "csv" => Format::Csv,
            "code" => Format::Code,
            _ => Format::Text,
        }
    }
}

/// Kết quả rút chữ.
#[derive(Clone, Debug)]
pub struct Extracted {
    pub text: String,
    pub format: Format,
    /// Tên hiển thị. Ưu tiên tiêu đề nằm *trong* tài liệu, vì `bao-cao-final-v3.docx`
    /// không nói cho ai biết tài liệu đó nói về cái gì.
    pub title: String,
}

/// Đuôi tệp được coi là mã nguồn hoặc dữ liệu có cấu trúc — đọc thẳng, không xử lý.
const CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "c", "h", "cc", "cpp",
    "hpp", "cs", "rb", "php", "sh", "bash", "zsh", "sql", "toml", "yaml", "yml", "json", "ini",
    "cfg", "xml", "lua", "r", "m", "scala", "dart", "vue", "svelte",
];

/// Định dạng suy từ đuôi tệp, hoặc `None` khi thư viện không đọc được tệp này.
///
/// Công khai vì lần quét thư mục phải hỏi đúng câu hỏi này **trước** khi mở tệp: một thư
/// mục ảnh mười nghìn tệp không được biến thành mười nghìn lần đọc rồi mười nghìn lần từ
/// chối. Đây cũng là chỗ duy nhất định nghĩa "tệp thư viện nhận", nên phép lọc lúc quét và
/// phép kiểm lúc nạp không bao giờ nói hai điều khác nhau về cùng một tệp.
pub fn format_for(path: &Path) -> Option<Format> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(Format::Pdf),
        "docx" => Some(Format::Docx),
        "md" | "markdown" | "mdx" => Some(Format::Markdown),
        "html" | "htm" | "xhtml" => Some(Format::Html),
        "csv" | "tsv" => Some(Format::Csv),
        "txt" | "text" | "log" | "rst" | "adoc" | "org" => Some(Format::Text),
        // Tệp không có đuôi — `README`, `Makefile`, `Dockerfile` — vẫn là văn bản, và từ
        // chối chúng chỉ vì thiếu một dấu chấm là từ chối đúng nhóm tệp hay bị kéo vào.
        "" => Some(Format::Text),
        other if CODE_EXTENSIONS.contains(&other) => Some(Format::Code),
        _ => None,
    }
}

/// Rút chữ từ một tệp trên đĩa.
pub fn extract(path: &Path) -> Result<Extracted, RagError> {
    let shown = path.display().to_string();
    let format = format_for(path).ok_or_else(|| RagError::Unsupported(shown.clone()))?;

    let meta = std::fs::metadata(path).map_err(|err| RagError::io(&shown, err))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(RagError::TooLarge {
            path: shown,
            bytes: meta.len(),
            limit: MAX_FILE_BYTES,
        });
    }

    let bytes = std::fs::read(path).map_err(|err| RagError::io(&shown, err))?;
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| shown.clone());

    let text = match format {
        Format::Pdf => from_pdf(&shown, &bytes)?,
        Format::Docx => from_docx(&shown, &bytes)?,
        Format::Html => from_html(&as_text(&shown, bytes)?),
        // Markdown, CSV, mã nguồn và văn bản thuần đều đã là chữ rồi. Đi qua một bộ phân
        // tích cú pháp chỉ để lấy lại đúng cái vừa đọc là mất mát ròng: bộ phân tích nào
        // cũng có một lớp tệp mà nó đọc hỏng.
        _ => as_text(&shown, bytes)?,
    };

    // Rút chữ "thành công" mà không ra chữ nào là một thất bại, và nó phải được gọi tên ở
    // đây. Xem [`RagError::Empty`]: đi tiếp thì tài liệu vào kho với 0 đoạn và không lý
    // do, rồi nằm ở "đang xếp hàng" mãi mãi. Đếm ký tự không phải khoảng trắng chứ không
    // phải `is_empty`, vì đúng trường hợp hay gặp nhất — PDF mà bảng mã font không đọc
    // được — trả về hàng trăm nghìn dấu cách và xuống dòng, tức là `is_empty()` bằng
    // `false` trên một tài liệu rỗng.
    if !text.chars().any(|c| !c.is_whitespace()) {
        return Err(RagError::empty(&shown, empty_reason(format)));
    }

    Ok(Extracted {
        title: title_of(format, &text, &stem),
        text,
        format,
    })
}

/// Gỡ một thực thể XML (`&amp;`, `&#233;`) thành ký tự của nó.
///
/// `BytesRef::decode` trả về **tên trần** — `amp`, `#233` — nên phải bọc lại thành dạng
/// đầy đủ trước khi nhờ `unescape` tra. Thực thể lạ (một DTD tự khai) không tra được;
/// bỏ qua nó chứ không chèn tên trần vào văn bản, vì `amp` nằm giữa một câu là một từ
/// sai chứ không phải một ký tự thiếu.
fn push_entity(out: &mut String, entity: &quick_xml::events::BytesRef<'_>) {
    let Ok(name) = entity.decode() else { return };
    match quick_xml::escape::unescape(&format!("&{name};")) {
        Ok(text) => out.push_str(&text),
        Err(err) => tracing::debug!(%err, %name, "bỏ qua thực thể XML không tra được"),
    }
}

/// Cùng phép thử với `pai_fs::path::looks_binary` — xem [`BINARY_PROBE`].
pub fn looks_binary(head: &[u8]) -> bool {
    head.iter().take(BINARY_PROBE).any(|byte| *byte == 0)
}

fn as_text(shown: &str, bytes: Vec<u8>) -> Result<String, RagError> {
    if looks_binary(&bytes) {
        return Err(RagError::Binary(shown.to_string()));
    }
    // `from_utf8_lossy` chứ không phải `from_utf8`: đến đây tệp đã qua cửa NUL, nên vài
    // byte lệch gần như luôn là một tệp Latin-1 hay UTF-16 lẻ tẻ chứ không phải ảnh. Ném
    // cả tài liệu đi vì một byte hỏng là đánh đổi sai.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// PDF, qua `pdf-extract`, có lưới bắt hoảng loạn.
///
/// `catch_unwind` ở đây không phải là sự cẩn thận thừa: `pdf-extract` gọi `panic!` trên
/// nhiều dạng PDF dị thường — font thiếu bảng mã, xref sai, luồng nén cụt — và đó chính
/// là loại tệp mà người ta tải từ Internet về rồi kéo vào. `AssertUnwindSafe` an toàn vì
/// hàm chỉ đọc một lát byte và trả về một `String`; không có trạng thái dùng chung nào ở
/// lại nửa vời sau khi hoảng loạn.
fn from_pdf(shown: &str, bytes: &[u8]) -> Result<String, RagError> {
    let attempt = catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }));
    match attempt {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(err)) => Err(RagError::extract(shown, err)),
        Err(_) => Err(RagError::extract(
            shown,
            "PDF dị dạng làm bộ rút chữ hoảng loạn; tệp đã bị bỏ qua để phần còn lại vẫn nạp được",
        )),
    }
}

/// DOCX: một tệp zip, phần chữ nằm ở `word/document.xml`.
///
/// Chỉ đọc đúng một mục trong zip. Header và footer (`word/header1.xml`, …) cố tình bị bỏ
/// qua: chúng lặp lại trên mọi trang, nên nạp vào là nhân bản cùng một câu lên vài chục
/// đoạn và đẩy nội dung thật xuống dưới trong mọi kết quả tìm kiếm.
fn from_docx(shown: &str, bytes: &[u8]) -> Result<String, RagError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| RagError::extract(shown, format!("không mở được dạng zip: {err}")))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|err| RagError::extract(shown, format!("thiếu word/document.xml — {err}")))?
        .read_to_string(&mut xml)
        .map_err(|err| RagError::extract(shown, err))?;
    Ok(docx_body(&xml))
}

/// Lọc text node của WordprocessingML.
///
/// So khớp bằng `local_name()` chứ không bằng tên đầy đủ: tiền tố namespace là `w:` ở mọi
/// tệp Word từng gặp, nhưng nó là **quy ước chứ không phải bắt buộc** — một bộ sinh DOCX
/// khác có quyền khai `wp:` hay không tiền tố, và lúc đó một phép so `b"w:t"` trả về đúng
/// một tài liệu rỗng mà không có lỗi nào.
fn docx_body(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                if tag.local_name().as_ref() == b"t" {
                    in_text = true;
                }
            }
            Ok(Event::End(tag)) => match tag.local_name().as_ref() {
                b"t" => in_text = false,
                // Ranh giới đoạn là thông tin thật của tài liệu, và phần cắt đoạn ở
                // `chunk` cắt theo nó. Mất xuống dòng ở đây là biến cả tệp thành một câu.
                b"p" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Empty(tag)) => match tag.local_name().as_ref() {
                b"tab" => out.push('\t'),
                b"br" | b"cr" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Text(text)) if in_text => {
                if let Ok(raw) = text.decode() {
                    out.push_str(&raw);
                }
            }
            // quick-xml tách `&amp;` ra thành **một sự kiện riêng**, không gộp vào `Text`.
            // Một nhánh `_ => {}` bắt trọn nó, và triệu chứng là mọi dấu `&` trong tài
            // liệu lặng lẽ biến mất — không lỗi, không cảnh báo, chỉ là một câu thiếu chữ.
            Ok(Event::GeneralRef(entity)) if in_text => push_entity(&mut out, &entity),
            Ok(Event::Eof) => break,
            // XML hỏng giữa chừng: giữ phần đã đọc được thay vì trả rỗng. Nửa tài liệu
            // vẫn tìm được, còn một lỗi ở đây chỉ nói với người dùng rằng tệp "không nạp
            // được" mà không nói phần nào.
            Err(err) => {
                tracing::debug!(%err, "word/document.xml hỏng giữa chừng, giữ phần đã đọc");
                break;
            }
            _ => {}
        }
    }
    out
}

/// HTML: bỏ thẻ, và bỏ **hẳn** nội dung của `<script>` và `<style>`.
///
/// Bỏ hẳn chứ không chỉ bỏ thẻ, vì mã JavaScript và luật CSS là chữ hợp lệ theo mọi phép
/// thử: chúng sẽ vào FTS5, chiếm chỗ trong đoạn được nhúng, và trả về như một trích dẫn
/// từ tài liệu. Một trang tin bình thường có nhiều byte script hơn byte bài viết.
fn from_html(html: &str) -> String {
    let mut reader = Reader::from_str(html);
    // HTML không phải XML: thẻ tự đóng, thẻ không đóng và thẻ đóng thừa là chuyện thường.
    // Bắt đúng luật XML ở đây nghĩa là dừng lại ở trang thật đầu tiên gặp phải.
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;

    let mut out = String::new();
    let mut skipping = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let name = tag.local_name();
                if is_opaque(name.as_ref()) {
                    skipping += 1;
                } else if breaks_line(name.as_ref()) {
                    out.push('\n');
                }
            }
            Ok(Event::End(tag)) => {
                let name = tag.local_name();
                if is_opaque(name.as_ref()) {
                    skipping = skipping.saturating_sub(1);
                } else if breaks_line(name.as_ref()) {
                    out.push('\n');
                }
            }
            Ok(Event::Empty(tag)) => {
                if matches!(tag.local_name().as_ref(), b"br" | b"hr") {
                    out.push('\n');
                }
            }
            Ok(Event::Text(text)) if skipping == 0 => {
                if let Ok(raw) = text.decode() {
                    out.push_str(&raw);
                }
            }
            Ok(Event::GeneralRef(entity)) if skipping == 0 => push_entity(&mut out, &entity),
            Ok(Event::Eof) => break,
            Err(err) => {
                tracing::debug!(%err, "HTML hỏng giữa chừng, giữ phần đã đọc");
                break;
            }
            _ => {}
        }
    }
    squeeze(&out)
}

fn is_opaque(name: &[u8]) -> bool {
    matches!(name, b"script" | b"style" | b"noscript" | b"template")
}

fn breaks_line(name: &[u8]) -> bool {
    matches!(
        name,
        b"p" | b"div"
            | b"section"
            | b"article"
            | b"li"
            | b"tr"
            | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"blockquote"
            | b"pre"
            | b"td"
            | b"th"
    )
}

/// Gộp khoảng trắng thừa mà việc bỏ thẻ để lại.
///
/// Không gộp thì mỗi thẻ thụt đầu dòng trong HTML nguồn biến thành một dòng trắng, và
/// phần cắt đoạn coi mỗi dòng trắng là một ranh giới — kết quả là hàng trăm đoạn rỗng.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Tiêu đề: chữ đầu tiên trong tài liệu nếu nó trông như một tiêu đề, nếu không thì tên tệp.
///
/// Ngưỡng 120 ký tự loại đúng trường hợp hay sai: một tệp `.txt` không có tiêu đề, và
/// dòng đầu của nó là câu mở bài dài ba dòng. Lấy nó làm tên hiển thị thì danh sách tài
/// liệu thành một bức tường chữ.
/// Vì sao một tệp mở được lại không cho chữ nào — nói theo đúng định dạng của nó.
///
/// Câu này đi thẳng lên huy hiệu tài liệu ở giao diện, nên nó phải nói được **bước tiếp
/// theo**. "Không rút được chữ" đúng nhưng để người dùng đứng yên; "bản quét ảnh, cần
/// OCR" thì họ biết mình đang cầm cái gì.
fn empty_reason(format: Format) -> &'static str {
    match format {
        Format::Pdf => {
            "PDF này không có lớp chữ nào đọc được — thường là bản quét ảnh, và thư viện \
             chưa có OCR nên chưa nạp được nội dung của nó"
        }
        Format::Docx => "tệp Word mở ra được nhưng phần thân không có chữ nào",
        _ => "tệp rỗng, hoặc chỉ có khoảng trắng",
    }
}

fn title_of(format: Format, text: &str, stem: &str) -> String {
    const MAX_TITLE: usize = 120;
    let first = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let candidate = match format {
        Format::Markdown => first.trim_start_matches('#').trim(),
        Format::Pdf | Format::Docx | Format::Html => first,
        // Với CSV dòng đầu là hàng tiêu đề cột, với mã nguồn là một `use` — cả hai đều là
        // tên hiển thị tệ hơn tên tệp.
        _ => "",
    };
    if candidate.is_empty() || candidate.chars().count() > MAX_TITLE {
        stem.to_string()
    } else {
        candidate.to_string()
    }
}
