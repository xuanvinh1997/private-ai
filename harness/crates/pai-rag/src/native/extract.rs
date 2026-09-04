use std::{
    io::{Cursor, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

use quick_xml::{Reader, events::Event};

use crate::{Format, RagError};

pub const EXTRACT_VERSION: u32 = 2;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const BINARY_PROBE: usize = 4_096;

const TEXT: &[&str] = &["txt", "text", "log", "rst", "adoc", "org"];
const MARKDOWN: &[&str] = &["md", "markdown", "mdx"];
const DATA: &[&str] = &["csv", "tsv", "json", "xml", "yaml", "yml"];
const HTML: &[&str] = &["html", "htm", "xhtml"];
const CODE: &[&str] = &[
    "py", "rs", "ts", "tsx", "js", "jsx", "go", "java", "kt", "c", "h", "cpp", "hpp", "cs", "rb",
    "php", "swift", "sh", "ps1", "sql", "toml", "ini", "cfg", "conf", "gradle", "lua", "r", "m",
    "scala", "dart",
];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];
const UNSUPPORTED_BINARY: &[&str] = &["xlsx", "xlsm", "pptx", "doc", "xls", "ppt"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderKind {
    Native(Format),
    Pdf,
    Image,
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct Extracted {
    pub text: String,
    pub format: Format,
    pub title: String,
    pub pages: u32,
    pub ocr_pages: Vec<u32>,
}

pub fn reader_for(path: &Path) -> Option<ReaderKind> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let ext = extension.as_str();
    if ext == "pdf" {
        Some(ReaderKind::Pdf)
    } else if IMAGE.contains(&ext) {
        Some(ReaderKind::Image)
    } else if ext == "docx" {
        Some(ReaderKind::Native(Format::Office))
    } else if MARKDOWN.contains(&ext) {
        Some(ReaderKind::Native(Format::Markdown))
    } else if HTML.contains(&ext) {
        Some(ReaderKind::Native(Format::Html))
    } else if DATA.contains(&ext) {
        Some(ReaderKind::Native(Format::Data))
    } else if CODE.contains(&ext) {
        Some(ReaderKind::Native(Format::Code))
    } else if TEXT.contains(&ext) || ext.is_empty() {
        Some(ReaderKind::Native(Format::Text))
    } else if UNSUPPORTED_BINARY.contains(&ext) {
        Some(ReaderKind::Unsupported)
    } else {
        None
    }
}

pub fn extract(path: &Path, format: Format) -> Result<Extracted, RagError> {
    let shown = path.display().to_string();
    let metadata = std::fs::metadata(path)
        .map_err(|error| RagError::Extraction(format!("không đọc được `{shown}`: {error}")))?;
    if !metadata.is_file() {
        return Err(RagError::Extraction(format!(
            "`{shown}` không phải một tệp"
        )));
    }
    if metadata.len() == 0 {
        return Err(RagError::Extraction(format!("`{shown}` là tệp rỗng")));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(RagError::Extraction(format!(
            "`{shown}` vượt trần {} MB",
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| RagError::Extraction(format!("không đọc được `{shown}`: {error}")))?;
    let text = match format {
        Format::Office => docx(&shown, &bytes)?,
        Format::Html => html(&String::from_utf8_lossy(&bytes)),
        _ => {
            if looks_binary(&bytes) {
                return Err(RagError::Extraction(format!(
                    "`{shown}` trông như tệp nhị phân dù mang đuôi văn bản"
                )));
            }
            decode_text(&bytes).trim().to_owned()
        }
    };
    if !text.chars().any(|character| !character.is_whitespace()) {
        return Err(RagError::Extraction(format!(
            "`{shown}` không có nội dung chữ"
        )));
    }
    Ok(Extracted {
        text,
        format,
        title: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or(shown),
        pages: 0,
        ocr_pages: Vec::new(),
    })
}

/// Extract a PDF text layer in Rust. This strict entry point is kept for callers and tests that do not provide
/// a vision model; the native library uses [`pdf_text_pages`] and falls back to OCR for sparse pages.
#[cfg(test)]
pub fn extract_pdf(path: &Path, min_chars_per_page: usize) -> Result<Extracted, RagError> {
    let pages = pdf_text_pages(path)?;
    if average_chars(&pages) < min_chars_per_page {
        return Err(RagError::Extraction(format!(
            "PDF `{}` không có đủ lớp chữ; cần bật OCR và chọn mô hình vision",
            path.display()
        )));
    }
    Ok(pdf_from_pages(path, pages, Vec::new()))
}

pub fn pdf_text_pages(path: &Path) -> Result<Vec<String>, RagError> {
    let shown = path.display().to_string();
    let metadata = std::fs::metadata(path)
        .map_err(|error| RagError::Extraction(format!("không đọc được `{shown}`: {error}")))?;
    if metadata.len() == 0 {
        return Err(RagError::Extraction(format!("`{shown}` là tệp rỗng")));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(RagError::Extraction(format!(
            "`{shown}` vượt trần {} MB",
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    let pages = match catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_by_pages(path)
    })) {
        Ok(Ok(pages)) => pages,
        Ok(Err(error)) => {
            return Err(RagError::Extraction(format!(
                "không mở được PDF `{shown}`: {error}"
            )));
        }
        Err(_) => {
            return Err(RagError::Extraction(format!(
                "PDF `{shown}` dị dạng làm bộ rút chữ dừng bất thường"
            )));
        }
    };
    if pages.is_empty() {
        return Err(RagError::Extraction(format!(
            "PDF `{shown}` không có trang nào"
        )));
    }
    Ok(pages)
}

pub fn average_chars(pages: &[String]) -> usize {
    if pages.is_empty() {
        return 0;
    }
    pages
        .iter()
        .map(|page| page.trim().chars().count())
        .sum::<usize>()
        / pages.len()
}

pub fn pdf_from_pages(path: &Path, pages: Vec<String>, ocr_pages: Vec<u32>) -> Extracted {
    let shown = path.display().to_string();
    let text = pages
        .iter()
        .enumerate()
        .map(|(index, page)| format!("<!-- pai-page:{} -->\n\n{}", index + 1, page.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    Extracted {
        text,
        format: Format::Pdf,
        title: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or(shown),
        pages: pages.len() as u32,
        ocr_pages,
    }
}

/// Render at most `limit` PDF pages into PNGs. PDFium is downloaded and cached on the first OCR request;
/// keeping this blocking function outside the async path prevents both FFI and PNG encoding from stalling it.
pub fn render_pdf_pages(path: &Path, scale: f32, limit: usize) -> Result<Vec<Vec<u8>>, RagError> {
    use image::ImageFormat;
    use pdfium_bundled::pdfium_render::prelude::*;

    let pdfium = pdfium_bundled::bind_pdfium_silent().map_err(|error| {
        RagError::Extraction(format!("không chuẩn bị được bộ dựng trang PDFium: {error}"))
    })?;
    let document = pdfium.load_pdf_from_file(path, None).map_err(|error| {
        RagError::Extraction(format!("không dựng được PDF `{}`: {error}", path.display()))
    })?;
    let width = (1_000.0 * scale.clamp(1.0, 4.0)).round() as i32;
    let config = PdfRenderConfig::new().set_target_width(width);
    let mut output = Vec::new();
    for page in document.pages().iter().take(limit) {
        let image = page
            .render_with_config(&config)
            .and_then(|bitmap| bitmap.as_image())
            .map_err(|error| RagError::Extraction(format!("không dựng được trang PDF: {error}")))?;
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| {
                RagError::Extraction(format!("không mã hoá được trang PDF: {error}"))
            })?;
        output.push(png.into_inner());
    }
    Ok(output)
}

pub fn image_bytes(path: &Path) -> Result<(Vec<u8>, &'static str), RagError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        RagError::Extraction(format!("không đọc được `{}`: {error}", path.display()))
    })?;
    if metadata.len() == 0 || metadata.len() > MAX_FILE_BYTES {
        return Err(RagError::Extraction(format!(
            "ảnh `{}` rỗng hoặc vượt trần {} MB",
            path.display(),
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    };
    let bytes = std::fs::read(path).map_err(|error| {
        RagError::Extraction(format!("không đọc được `{}`: {error}", path.display()))
    })?;
    Ok((bytes, mime))
}

pub fn scan(root: &Path, limit: usize) -> (Vec<PathBuf>, usize) {
    let mut files = Vec::new();
    let mut over_limit = 0;
    let mut stack = vec![root.to_owned()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || skip_dir(&name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && reader_for(&path).is_some() {
                if files.len() < limit {
                    files.push(path);
                } else {
                    over_limit += 1;
                }
            }
        }
    }
    files.sort();
    (files, over_limit)
}

fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".venv"
            | "venv"
            | "env"
            | "node_modules"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".ruff_cache"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | ".cache"
            | ".idea"
            | ".vscode"
            | ".tox"
            | "site-packages"
            | ".gradle"
            | ".terraform"
            | "vendor"
            | ".DS_Store"
            | "$RECYCLE.BIN"
    )
}

fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(BINARY_PROBE).any(|byte| *byte == 0)
}

fn decode_text(data: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_owned();
    }
    for encoding in [
        encoding_rs::Encoding::for_label(b"windows-1258"),
        Some(encoding_rs::WINDOWS_1252),
    ]
    .into_iter()
    .flatten()
    {
        let (text, had_errors) = encoding.decode_without_bom_handling(data);
        if !had_errors {
            return text.into_owned();
        }
    }
    String::from_utf8_lossy(data).into_owned()
}

fn docx(shown: &str, bytes: &[u8]) -> Result<String, RagError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| RagError::Extraction(format!("không mở được DOCX `{shown}`: {error}")))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|error| RagError::Extraction(format!("DOCX `{shown}` thiếu nội dung: {error}")))?
        .read_to_string(&mut xml)
        .map_err(|error| RagError::Extraction(format!("không đọc được DOCX `{shown}`: {error}")))?;
    Ok(xml_text(&xml, true))
}

fn html(source: &str) -> String {
    xml_text(source, false)
}

fn xml_text(source: &str, docx: bool) -> String {
    let mut reader = Reader::from_str(source);
    if !docx {
        reader.config_mut().check_end_names = false;
        reader.config_mut().allow_unmatched_ends = true;
    }
    let mut output = String::new();
    let mut in_word_text = !docx;
    let mut skipping = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let name = tag.local_name();
                if docx && name.as_ref() == b"t" {
                    in_word_text = true;
                } else if !docx && opaque(name.as_ref()) {
                    skipping += 1;
                } else if line_break(name.as_ref(), docx) {
                    output.push('\n');
                }
            }
            Ok(Event::End(tag)) => {
                let name = tag.local_name();
                if docx && name.as_ref() == b"t" {
                    in_word_text = false;
                } else if !docx && opaque(name.as_ref()) {
                    skipping = skipping.saturating_sub(1);
                } else if line_break(name.as_ref(), docx) {
                    output.push('\n');
                }
            }
            Ok(Event::Empty(tag))
                if matches!(tag.local_name().as_ref(), b"br" | b"cr" | b"tab") =>
            {
                output.push(if tag.local_name().as_ref() == b"tab" {
                    '\t'
                } else {
                    '\n'
                });
            }
            Ok(Event::Text(text)) if in_word_text && skipping == 0 => {
                if let Ok(decoded) = text.decode() {
                    output.push_str(&decoded);
                }
            }
            Ok(Event::GeneralRef(entity)) if in_word_text && skipping == 0 => {
                if let Ok(name) = entity.decode()
                    && let Ok(decoded) = quick_xml::escape::unescape(&format!("&{name};"))
                {
                    output.push_str(&decoded);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    output
}

fn opaque(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style")
}

fn line_break(name: &[u8], docx: bool) -> bool {
    if docx {
        name == b"p"
    } else {
        [
            b"p".as_slice(),
            b"div",
            b"section",
            b"article",
            b"li",
            b"h1",
            b"h2",
            b"h3",
            b"h4",
            b"h5",
            b"h6",
            b"tr",
        ]
        .iter()
        .any(|tag| name.eq_ignore_ascii_case(tag))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn html_drops_script_and_keeps_structure() {
        let text = html("<h1>Tiêu đề</h1><script>ignore()</script><p>Nội dung &amp; bảng.</p>");
        assert!(text.contains("Tiêu đề"));
        assert!(text.contains("Nội dung & bảng."));
        assert!(!text.contains("ignore"));
    }

    #[test]
    fn supported_formats_choose_native_or_report_unsupported() {
        assert_eq!(
            reader_for(Path::new("a.md")),
            Some(ReaderKind::Native(Format::Markdown))
        );
        assert_eq!(reader_for(Path::new("a.pdf")), Some(ReaderKind::Pdf));
        assert_eq!(reader_for(Path::new("a.png")), Some(ReaderKind::Image));
        assert_eq!(reader_for(Path::new("a.exe")), None);
    }

    #[test]
    fn decodes_legacy_vietnamese_windows_text() {
        assert_eq!(decode_text(&[b'T', 0xe0, b'i']), "Tài");
    }

    #[test]
    fn docx_extracts_paragraphs_and_entities() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(
                br#"<w:document xmlns:w="urn:w"><w:body><w:p><w:r><w:t>Cai dat &amp; cau hinh</w:t></w:r></w:p><w:p><w:r><w:t>dong hai</w:t></w:r></w:p></w:body></w:document>"#,
            )
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let text = docx("fixture.docx", &bytes).unwrap();
        assert!(text.contains("Cai dat & cau hinh"));
        assert!(text.contains("dong hai"));
        assert!(text.contains('\n'));
    }

    #[test]
    fn pdf_text_layer_keeps_page_metadata() {
        let pdf = one_page_pdf("Native Rust PDF text layer");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.pdf");
        std::fs::write(&path, pdf).unwrap();

        let extracted = extract_pdf(&path, 1).unwrap();
        assert_eq!(extracted.format, Format::Pdf);
        assert_eq!(extracted.pages, 1);
        assert!(extracted.text.contains("<!-- pai-page:1 -->"));
        assert!(extracted.text.contains("Native Rust PDF text layer"));
        assert!(matches!(
            extract_pdf(&path, 1_000),
            Err(RagError::Extraction(_))
        ));
    }

    #[test]
    #[ignore = "downloads and binds PDFium"]
    fn pdfium_renders_a_page_for_ocr() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scan.pdf");
        std::fs::write(&path, one_page_pdf("OCR render probe")).unwrap();
        let pages = render_pdf_pages(&path, 1.0, 1).unwrap();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    fn one_page_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 12 Tf 72 700 Td ({text}) Tj ET\n");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn malformed_pdf_isolated_as_one_extraction_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.pdf");
        std::fs::write(&path, b"%PDF-1.4\ntruncated").unwrap();
        assert!(matches!(
            extract_pdf(&path, 1),
            Err(RagError::Extraction(_))
        ));
    }
}
