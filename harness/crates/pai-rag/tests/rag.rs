//! Những bất biến mà thư viện tài liệu sai thì mô hình trích dẫn nhầm chỗ — hoặc không
//! trích dẫn được gì cả.
//!
//! Mỗi bài ở đây khoá một câu đã viết trong tài liệu crate. Nếu một bài đỏ thì hoặc mã
//! sai, hoặc câu trong tài liệu đã hết đúng — không có khả năng thứ ba.
//!
//! Ba chỗ cố tình **không** mock: SQLite là thật (kể cả FTS5 và trigger), tệp là thật
//! (DOCX được nén bằng chính crate `zip`, PDF được dựng đủ xref), và sổ đăng ký tool là
//! thật. Thứ duy nhất được thay bằng bản giả là **bộ nhúng**, vì cái đáng kiểm ở đó là
//! phép hợp nhất RRF chứ không phải chất lượng của một mô hình.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use pai_core::{Context, Plugin};
use pai_rag::chunk::{Chunk, ChunkOpts, chunk};
use pai_rag::embed::Embedder;
use pai_rag::error::RagError;
use pai_rag::extract::{Format, extract};
use pai_rag::library::{DocLibrary, Docs, IngestStage, Library};
use pai_rag::plugin::RagPlugin;
use pai_rag::search::MatchedBy;
use pai_rag::store::Store;
use pai_tools::{Invocation, Resolution, Tools, ToolsPlugin, UNTRUSTED_NOTICE};
use tempfile::TempDir;

// ---------------------------------------------------------------------------------
// Đồ nghề
// ---------------------------------------------------------------------------------

/// Một văn bản tiếng Việt có dấu, đủ dài để phải cắt thành nhiều đoạn.
///
/// Chữ có dấu là **điều kiện của bài**, không phải trang trí: `ế` chiếm ba byte, nên một
/// phép cắt tính bằng chỉ số byte sẽ rơi vào giữa nó và làm cả tiến trình hoảng loạn.
/// Một văn bản mẫu bằng tiếng Anh không bao giờ chạm tới lỗi đó.
const VAN_BAN_TIENG_VIET: &str = "\
# Sổ tay bảo mật nội bộ

Tài liệu này mô tả cách chúng tôi giữ khoá bí mật và chứng chỉ. Mọi khoá đều được sinh \
trên máy của người dùng, và không có đường nào để chúng rời khỏi thiết bị. Điều đó nghe \
như một chi tiết kỹ thuật, nhưng nó là lời hứa trung tâm của sản phẩm.

Khi một khoá hết hạn, hệ thống sẽ nhắc trước ba mươi ngày. Nhắc sớm hơn thì người ta bỏ \
qua; nhắc muộn hơn thì họ không kịp xoay xở. Ba mươi ngày là chỗ hai điều đó gặp nhau.

## Chứng chỉ và vòng đời của chúng

Chứng chỉ được ký bằng một khoá trung gian, và khoá trung gian đó nằm trong một két \
riêng. Ai cũng đọc được chứng chỉ; không ai đọc được khoá. Đây là cách duy nhất khiến \
việc chia sẻ tài liệu không kéo theo việc chia sẻ quyền.

Mỗi lần gia hạn đều được ghi vào sổ. Sổ chỉ ghi thêm, không sửa và không xoá, vì một bản \
ghi sửa được là một bản ghi không chứng minh được điều gì.

## Những chỗ hay sai

Người ta hay chép khoá vào biến môi trường rồi quên mất rằng biến môi trường đi vào nhật \
ký của mọi công cụ gỡ lỗi đang mở. Đó là lý do phần cấu hình không nhận khoá qua biến môi \
trường, dù làm thế sẽ tiện hơn nhiều cho người viết mã.

Chỗ sai thứ hai là tin vào tên tệp. Một tệp tên `chung-chi-cong-khai.pem` hoàn toàn có \
thể chứa một khoá riêng, và một lần nhìn nhầm là một lần rò rỉ.
";

/// Bộ nhúng giả: túi từ trên một bảng từ vựng nhỏ, tất định tuyệt đối.
///
/// Bảng từ vựng được dựng để tách ba trường hợp ra khỏi nhau, và đó là toàn bộ lý do nó
/// tồn tại:
///
/// - `mèo` và `mimi` **cùng một chiều** — hai từ khác nhau, gần nhau về nghĩa. Một tài
///   liệu chỉ có `mimi` được tìm ra bằng ngữ nghĩa nhưng không bằng từ khoá.
/// - `chuột` **không có trong bảng** — bộ nhúng không biết từ đó. Một tài liệu chỉ có
///   `chuột` được tìm ra bằng từ khoá nhưng vector của nó là vector không, nên cosine
///   loại nó ra.
///
/// Đây cũng là hành vi thật của một bộ nhúng nhỏ: nó biết một số khái niệm và mù trước
/// những khái niệm khác.
struct TuiTuGia {
    tu_vung: HashMap<&'static str, usize>,
    so_chieu: usize,
}

impl TuiTuGia {
    fn moi() -> TuiTuGia {
        let mut tu_vung = HashMap::new();
        tu_vung.insert("mèo", 0usize);
        tu_vung.insert("mimi", 0usize);
        tu_vung.insert("nằm", 1usize);
        tu_vung.insert("ghế", 2usize);
        tu_vung.insert("chiếu", 3usize);
        TuiTuGia {
            tu_vung,
            so_chieu: 8,
        }
    }

    fn vector(&self, text: &str) -> Vec<f32> {
        let mut out = vec![0.0f32; self.so_chieu];
        for tu in text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|tu| !tu.is_empty())
        {
            if let Some(chieu) = self.tu_vung.get(tu) {
                out[*chieu] += 1.0;
            }
        }
        out
    }
}

#[async_trait]
impl Embedder for TuiTuGia {
    fn id(&self) -> &str {
        "tui-tu-gia"
    }

    fn dim(&self) -> Option<usize> {
        Some(self.so_chieu)
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        Ok(texts.iter().map(|text| self.vector(text)).collect())
    }

    async fn health(&self) -> bool {
        true
    }
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

/// Dựng một tệp `.docx` thật bằng chính crate `zip` — không có tệp mẫu nào được chép vào
/// repo. Một tệp nhị phân nằm trong thư mục test là một thứ không ai đọc lại được và
/// không ai sửa được khi định dạng đổi.
fn docx_toi_gian(path: &Path, than_bai: &str) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("[Content_Types].xml", options).unwrap();
    writer.write_all(CONTENT_TYPES.as_bytes()).unwrap();
    writer.start_file("word/document.xml", options).unwrap();
    writer.write_all(than_bai.as_bytes()).unwrap();
    writer.finish().unwrap();
}

/// Một PDF hợp lệ, xref tính đúng theo vị trí thật của từng đối tượng.
///
/// Chữ phải là ASCII: font ở đây là Helvetica với bảng mã mặc định, và nhét chữ có dấu
/// vào đó chỉ tạo ra một bài kiểm chứng nói về bảng mã chứ không nói về việc rút chữ.
fn pdf_toi_gian(noi_dung: &str) -> Vec<u8> {
    let stream = format!("BT /F1 24 Tf 72 700 Td ({noi_dung}) Tj ET\n");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];

    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Nạp một loạt tệp và trả về dòng sự kiện đã thu hết.
async fn nap(library: &Library, paths: Vec<std::path::PathBuf>) -> Vec<pai_rag::IngestEvent> {
    library.ingest(paths).collect::<Vec<_>>().await
}

fn viet(dir: &Path, ten: &str, noi_dung: &str) -> std::path::PathBuf {
    let path = dir.join(ten);
    std::fs::write(&path, noi_dung).unwrap();
    path
}

// ---------------------------------------------------------------------------------
// 1. Cắt đoạn trên chữ có dấu
// ---------------------------------------------------------------------------------

/// Bẫy chính của `chunk`: `ế` chiếm ba byte, và một chỉ số byte tính bằng phép đếm ký tự
/// sẽ rơi vào giữa nó. Bài này khoá cả hai nửa của lời hứa — ranh giới hợp lệ, và không
/// mất chữ.
#[test]
fn cat_doan_tieng_viet_khong_lam_vo_ky_tu() {
    let opts = ChunkOpts::new(320, 60);
    let doan = chunk(VAN_BAN_TIENG_VIET, opts);
    assert!(
        doan.len() > 3,
        "văn bản mẫu phải cắt ra nhiều đoạn: {}",
        doan.len()
    );

    for c in &doan {
        assert!(
            VAN_BAN_TIENG_VIET.is_char_boundary(c.start),
            "đoạn #{} bắt đầu ở byte {} — không phải ranh giới ký tự",
            c.ord,
            c.start
        );
        assert!(
            VAN_BAN_TIENG_VIET.is_char_boundary(c.end),
            "đoạn #{} kết thúc ở byte {} — không phải ranh giới ký tự",
            c.ord,
            c.end
        );
        assert!(c.start < c.end, "đoạn #{} rỗng", c.ord);
        // Lát cắt phải khớp từng byte với chữ đã lưu: lệch một byte ở đây là trích dẫn
        // trỏ sai chỗ trong tài liệu gốc.
        assert_eq!(
            &VAN_BAN_TIENG_VIET[c.start..c.end],
            c.text,
            "đoạn #{} không khớp lát cắt gốc",
            c.ord
        );
    }

    // Số thứ tự phải liên tục — mô hình trích dẫn bằng con số này.
    for (index, c) in doan.iter().enumerate() {
        assert_eq!(c.ord, index as u32);
    }

    // Không mất chữ: mọi byte không phải khoảng trắng đều nằm trong ít nhất một đoạn.
    for (offset, ch) in VAN_BAN_TIENG_VIET.char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        assert!(
            doan.iter().any(|c| c.start <= offset && offset < c.end),
            "ký tự {ch:?} ở byte {offset} không nằm trong đoạn nào"
        );
    }

    // Và phải có chồng lấn thật: nó tồn tại để một câu vắt qua ranh giới vẫn nguyên vẹn ở
    // một trong hai đoạn.
    assert!(
        doan.windows(2).any(|cap| cap[1].start < cap[0].end),
        "không có đoạn nào chồng lấn đoạn trước"
    );
}

/// Tiêu đề markdown phải theo được xuống các đoạn bên dưới nó: "phần Chứng chỉ nói gì" là
/// câu hỏi mà chỉ nội dung đoạn không trả lời được.
#[test]
fn cat_doan_giu_tieu_de_muc() {
    let doan = chunk(VAN_BAN_TIENG_VIET, ChunkOpts::new(320, 60));
    let tieu_de: Vec<&str> = doan
        .iter()
        .filter_map(|c: &Chunk| c.heading.as_deref())
        .collect();
    assert!(
        tieu_de.contains(&"Chứng chỉ và vòng đời của chúng"),
        "{tieu_de:?}"
    );
    assert!(tieu_de.contains(&"Những chỗ hay sai"), "{tieu_de:?}");
}

/// Một "câu" dài hơn cả một đoạn — bảng biểu, khối mã — vẫn phải cắt được, và cắt cứng
/// cũng không được phép vỡ ký tự.
#[test]
fn cat_cung_mot_khoi_khong_co_dau_cham() {
    let khoi = "đường".repeat(400);
    let doan = chunk(&khoi, ChunkOpts::new(100, 10));
    assert!(doan.len() > 5, "{}", doan.len());
    for c in &doan {
        assert!(khoi.is_char_boundary(c.start) && khoi.is_char_boundary(c.end));
        assert_eq!(&khoi[c.start..c.end], c.text);
    }
}

// ---------------------------------------------------------------------------------
// 2. Rút chữ
// ---------------------------------------------------------------------------------

#[test]
fn rut_chu_tu_docx_that() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bao-cao-final-v3.docx");
    docx_toi_gian(
        &path,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
<w:p><w:r><w:t>Báo cáo bảo mật quý ba</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Đoạn thứ nhất nói về </w:t></w:r><w:r><w:t>khoá &amp; chứng chỉ.</w:t></w:r></w:p>
<w:p><w:r><w:t>Đoạn thứ hai nằm ở dòng khác.</w:t></w:r></w:p>
</w:body>
</w:document>"#,
    );

    let ra = extract(&path).unwrap();
    assert_eq!(ra.format, Format::Docx);
    assert!(ra.text.contains("Báo cáo bảo mật quý ba"), "{:?}", ra.text);
    // Hai `<w:t>` liền nhau trong cùng một `<w:p>` phải nối lại thành một câu.
    assert!(
        ra.text.contains("Đoạn thứ nhất nói về khoá & chứng chỉ."),
        "{:?}",
        ra.text
    );
    // Và ranh giới `<w:p>` phải thành xuống dòng, nếu không cả tệp là một câu duy nhất.
    assert!(
        ra.text.contains("chứng chỉ.\nĐoạn thứ hai"),
        "mất ranh giới đoạn: {:?}",
        ra.text
    );
    // Tiêu đề lấy từ trong tài liệu, không phải từ cái tên tệp vô nghĩa.
    assert_eq!(ra.title, "Báo cáo bảo mật quý ba");
}

#[test]
fn rut_chu_tu_html_bo_han_script_va_style() {
    let dir = TempDir::new().unwrap();
    let path = viet(
        dir.path(),
        "chinh-sach.html",
        r#"<html><head><style>body { color: #ff0000 }</style></head>
<body>
<h1>Chính sách riêng tư</h1>
<p>Nội dung &amp; điều khoản áp dụng.</p>
<script>var loi = "câu này không được vào chỉ mục";</script>
<div>Phần cuối của trang.</div>
</body></html>"#,
    );

    let ra = extract(&path).unwrap();
    assert_eq!(ra.format, Format::Html);
    assert!(ra.text.contains("Chính sách riêng tư"), "{:?}", ra.text);
    assert!(
        ra.text.contains("Nội dung & điều khoản áp dụng."),
        "{:?}",
        ra.text
    );
    assert!(ra.text.contains("Phần cuối của trang."), "{:?}", ra.text);
    // Nội dung script và style là chữ hợp lệ theo mọi phép thử; chỉ có luật riêng mới
    // giữ chúng ra ngoài.
    assert!(
        !ra.text.contains("câu này không được vào chỉ mục"),
        "script lọt vào: {:?}",
        ra.text
    );
    assert!(!ra.text.contains("#ff0000"), "style lọt vào: {:?}", ra.text);
    assert_eq!(ra.title, "Chính sách riêng tư");
}

#[test]
fn rut_chu_tu_csv() {
    let dir = TempDir::new().unwrap();
    let path = viet(
        dir.path(),
        "ton-kho.csv",
        "tên,số lượng,ghi chú\nbàn phím,3,còn bảo hành\nchuột,5,hết hàng\n",
    );

    let ra = extract(&path).unwrap();
    assert_eq!(ra.format, Format::Csv);
    assert!(ra.text.contains("bàn phím,3,còn bảo hành"), "{:?}", ra.text);
    // Dòng đầu của CSV là hàng tiêu đề cột, không phải tên tài liệu.
    assert_eq!(ra.title, "ton-kho");
}

#[test]
fn rut_chu_tu_pdf() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bao-cao.pdf");
    std::fs::write(&path, pdf_toi_gian("Bao cao ky thuat quy ba")).unwrap();

    let ra = extract(&path).unwrap();
    assert_eq!(ra.format, Format::Pdf);
    assert!(
        ra.text.contains("Bao cao ky thuat"),
        "không rút được chữ từ PDF: {:?}",
        ra.text
    );
}

/// `pdf-extract` **panic** trên cấu trúc dị dạng thay vì trả lỗi. Bài này khoá cái lưới
/// bắt hoảng loạn: một tệp hỏng phải thành một `Err` đọc được, không phải một tiến trình
/// chết mang theo mười chín tệp còn lại trong hàng đợi.
#[test]
fn pdf_hong_thanh_loi_chu_khong_giet_tien_trinh() {
    let dir = TempDir::new().unwrap();
    let mut hong = pdf_toi_gian("Bao cao ky thuat");
    hong.truncate(hong.len() / 2);
    let path = dir.path().join("cut-giua-chung.pdf");
    std::fs::write(&path, &hong).unwrap();

    let err = extract(&path).expect_err("PDF cụt phải là lỗi");
    assert!(
        matches!(err, RagError::Extract { .. }),
        "lỗi sai loại: {err}"
    );
    // Và thông báo phải gọi ra đúng tệp, nếu không người dùng không biết bỏ tệp nào ra.
    assert!(err.to_string().contains("cut-giua-chung.pdf"), "{err}");
}

// ---------------------------------------------------------------------------------
// 3. Từ chối tệp nhị phân
// ---------------------------------------------------------------------------------

/// Một tệp nhị phân đội lốt `.txt` phải bị chặn ở cùng ngưỡng mà `pai-fs` dùng — hai câu
/// trả lời trái nhau cho cùng một tệp là một lỗi không ai gỡ được.
#[test]
fn tu_choi_tep_nhi_phan() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("anh.txt");
    let mut bytes = b"GIF89a".to_vec();
    bytes.extend_from_slice(&[0u8; 64]);
    bytes.extend_from_slice("chữ nằm sau byte NUL".as_bytes());
    std::fs::write(&path, &bytes).unwrap();

    let err = extract(&path).expect_err("tệp có byte NUL phải bị từ chối");
    assert!(matches!(err, RagError::Binary(_)), "lỗi sai loại: {err}");

    // Và byte NUL nằm ngoài 4096 byte đầu thì **không** bị dò — đó chính là ngưỡng, và
    // một bài kiểm chứng chỉ khoá một chiều thì không khoá gì cả.
    let mut sach = vec![b'a'; 5000];
    sach.push(0);
    let path = dir.path().join("dai.txt");
    std::fs::write(&path, &sach).unwrap();
    assert!(extract(&path).is_ok());
}

// ---------------------------------------------------------------------------------
// 4. Nạp trùng
// ---------------------------------------------------------------------------------

/// Người dùng kéo cùng một tệp vào hai lần — lần thứ hai từ một thư mục khác. Hai hàng
/// giống hệt nhau trong danh sách là lỗi họ nhìn thấy ngay.
#[tokio::test]
async fn nap_cung_mot_tep_hai_lan_chi_ra_mot_tai_lieu() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let library = Library::open(kho.path(), None).unwrap();

    let lan_dau = nap(&library, vec![path.clone()]).await;
    assert!(lan_dau.iter().any(|e| e.stage == IngestStage::Stored));
    assert_eq!(library.documents().unwrap().len(), 1);
    let doan_lan_dau = library.stats().unwrap().chunks;

    // Cùng đường dẫn, lần thứ hai.
    nap(&library, vec![path.clone()]).await;
    assert_eq!(
        library.documents().unwrap().len(),
        1,
        "nạp lại cùng đường dẫn"
    );

    // Cùng nội dung, đường dẫn khác — đây mới là trường hợp mà so đường dẫn sẽ trượt.
    let ban_sao = viet(nguon.path(), "so-tay-copy.md", VAN_BAN_TIENG_VIET);
    nap(&library, vec![ban_sao]).await;

    let tai_lieu = library.documents().unwrap();
    assert_eq!(
        tai_lieu.len(),
        1,
        "cùng nội dung phải là một tài liệu: {tai_lieu:?}"
    );
    // Và đoạn không được nhân đôi theo.
    assert_eq!(library.stats().unwrap().chunks, doan_lan_dau);

    // Bản sao nằm trong kho của dự án, không trỏ về chỗ người dùng lấy nó: thư mục nguồn
    // có thể bị dọn bất cứ lúc nào.
    assert!(
        tai_lieu[0].path.starts_with(kho.path()),
        "{:?}",
        tai_lieu[0].path
    );
    assert!(tai_lieu[0].path.exists());
}

// ---------------------------------------------------------------------------------
// 5. Tìm khi không có bộ nhúng
// ---------------------------------------------------------------------------------

/// Bất biến trung tâm của cả crate: không có bộ nhúng thì việc nạp vẫn chạy tới cùng, tìm
/// bằng từ khoá vẫn ra kết quả, và `stats()` **nói ra** vì sao phần ngữ nghĩa chưa có.
#[tokio::test]
async fn khong_co_bo_nhung_van_tim_duoc_bang_tu_khoa() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let library = Library::open(kho.path(), None).unwrap();
    let su_kien = nap(&library, vec![path]).await;

    // Không có bộ nhúng **không** phải một lần nạp hỏng.
    let luu = su_kien
        .iter()
        .find(|e| e.stage == IngestStage::Stored)
        .expect("phải có sự kiện lưu");
    assert!(luu.error.is_none(), "{:?}", luu.error);
    assert!(su_kien.last().unwrap().finished);

    let hits = library.search("chứng chỉ", 5).await.unwrap();
    assert!(
        !hits.is_empty(),
        "FTS5 phải trả về kết quả khi không có vector"
    );
    assert!(hits.iter().all(|h| h.matched_by == MatchedBy::Keyword));

    // Hỏi không dấu cũng phải ra — người Việt gõ tìm kiếm không dấu suốt.
    assert!(!library.search("chung chi", 5).await.unwrap().is_empty());

    let stats = library.stats().unwrap();
    assert!(stats.documents == 1 && stats.chunks > 0);
    assert_eq!(stats.embedded_chunks, 0);
    assert!(stats.embedder.is_none());
    assert!(!stats.semantic_ready);
    let reason = stats.reason.expect("phải nói ra vì sao");
    assert!(
        reason.contains("nhúng") && reason.contains("từ khoá"),
        "lý do phải đọc được bằng tiếng Việt: {reason}"
    );
}

// ---------------------------------------------------------------------------------
// 6. Hợp nhất RRF với bộ nhúng giả
// ---------------------------------------------------------------------------------

/// Ba trường hợp của `matched_by`, và thứ hạng mà RRF phải cho ra.
///
/// Bố trí: câu hỏi `mèo chuột`.
///
/// | tài liệu | từ khoá | ngữ nghĩa | mong đợi |
/// |---|---|---|---|
/// | `Con mèo nằm trên chiếu.` | có (`mèo`) | có | `both` |
/// | `Mimi nằm trên ghế.`      | không      | có (`mimi` cùng chiều với `mèo`) | `semantic` |
/// | `Bầy chuột chạy quanh kho.` | có (`chuột`) | không (`chuột` ngoài từ vựng ⇒ vector không) | `keyword` |
///
/// Tài liệu đầu có mặt ở **cả hai** bảng xếp hạng, nên nó phải đứng nhất — đó chính là
/// điều mà cộng điểm thô không bảo đảm được và RRF thì có.
#[tokio::test]
async fn hop_nhat_rrf_cho_thu_hang_va_nhan_dung() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let a = viet(nguon.path(), "a.txt", "Con mèo nằm trên chiếu.");
    let b = viet(nguon.path(), "b.txt", "Mimi nằm trên ghế.");
    let c = viet(nguon.path(), "c.txt", "Bầy chuột chạy quanh kho.");

    let embedder: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
    let library = Library::open(kho.path(), Some(embedder)).unwrap();
    nap(&library, vec![a, b, c]).await;

    let stats = library.stats().unwrap();
    assert_eq!(stats.documents, 3);
    assert_eq!(
        stats.embedded_chunks, stats.chunks,
        "mọi đoạn phải có vector"
    );
    assert!(stats.semantic_ready, "{:?}", stats.reason);
    assert_eq!(stats.embedder.as_deref(), Some("tui-tu-gia"));

    let hits = library.search("mèo chuột", 10).await.unwrap();
    assert_eq!(hits.len(), 3, "{hits:#?}");

    let nhan = |chua: &str| -> MatchedBy {
        hits.iter()
            .find(|h| h.text.contains(chua))
            .unwrap_or_else(|| panic!("không thấy đoạn chứa {chua:?} trong {hits:#?}"))
            .matched_by
    };
    assert_eq!(nhan("Con mèo"), MatchedBy::Both);
    assert_eq!(nhan("Mimi"), MatchedBy::Semantic);
    assert_eq!(nhan("Bầy chuột"), MatchedBy::Keyword);

    // Đồng thuận giữa hai cách tìm là bằng chứng mạnh hơn một lần đứng nhất ở một cách.
    assert!(
        hits[0].text.contains("Con mèo"),
        "đoạn có mặt ở cả hai bảng phải đứng nhất: {hits:#?}"
    );
    assert!(hits[0].score > hits[1].score);
    // Và điểm phải là điểm RRF, không phải điểm thô của một bên.
    assert!(
        hits[0].score > 1.0 / (pai_rag::RRF_K + 1.0),
        "điểm của đoạn `both` phải là tổng hai đóng góp: {}",
        hits[0].score
    );

    // Trích dẫn được: mỗi kết quả mang tên tài liệu và số thứ tự đoạn.
    assert!(hits.iter().all(|h| !h.title.is_empty()));
}

/// Bộ nhúng hỏng giữa chừng không được biến một lần tìm thành một lần hỏng — nửa từ khoá
/// vẫn phải trả lời được.
#[tokio::test]
async fn bo_nhung_chet_thi_lui_ve_tu_khoa_chu_khong_bao_loi() {
    struct LuonHong;

    #[async_trait]
    impl Embedder for LuonHong {
        fn id(&self) -> &str {
            "luon-hong"
        }
        fn dim(&self) -> Option<usize> {
            None
        }
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
            Err(RagError::Embed {
                id: "luon-hong".into(),
                reason: "máy chủ không trả lời".into(),
            })
        }
        async fn health(&self) -> bool {
            false
        }
    }

    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let embedder: Arc<dyn Embedder> = Arc::new(LuonHong);
    let library = Library::open(kho.path(), Some(embedder)).unwrap();
    let su_kien = nap(&library, vec![path]).await;

    // Nạp vẫn thành công: tài liệu đã vào FTS5.
    let luu = su_kien
        .iter()
        .find(|e| e.stage == IngestStage::Stored)
        .expect("phải có sự kiện lưu dù nhúng hỏng");
    assert!(luu.error.is_some(), "lý do phải được ghi lại");
    assert_eq!(library.documents().unwrap().len(), 1);

    // Và tìm vẫn ra kết quả.
    let hits = library.search("chứng chỉ", 5).await.unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.matched_by == MatchedBy::Keyword));

    let stats = library.stats().unwrap();
    assert!(!stats.semantic_ready);
    let reason = stats.reason.expect("phải nói ra vì sao");
    assert!(reason.contains("máy chủ không trả lời"), "{reason}");
}

// ---------------------------------------------------------------------------------
// 7. Xoá tài liệu
// ---------------------------------------------------------------------------------

/// Xoá phải kéo theo đoạn, hàng FTS **và** vector. Khẳng định bằng đếm hàng, vì đây đúng
/// là chỗ `ON DELETE CASCADE` lừa được người viết: SQLite không kích hoạt trigger cho
/// hàng bị xoá theo dây chuyền, nên chỉ mục FTS có thể ở lại với những hàng mồ côi mà
/// **vẫn trả về kết quả**.
#[tokio::test]
async fn xoa_tai_lieu_keo_theo_doan_hang_fts_va_vector() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let mot = viet(nguon.path(), "mot.md", VAN_BAN_TIENG_VIET);
    let hai = viet(
        nguon.path(),
        "hai.md",
        "# Ghi chép khác\n\nMột con hà mã đứng ngoài hiên và không nói gì cả.\n",
    );

    let embedder: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
    let library = Library::open(kho.path(), Some(embedder)).unwrap();
    nap(&library, vec![mot, hai]).await;

    let db = Store::open(
        &kho.path().join("library.sqlite"),
        &kho.path().join("files"),
    )
    .unwrap();

    let truoc = db.counts().unwrap();
    assert_eq!(truoc.documents, 2);
    assert!(truoc.chunks > 1);
    assert_eq!(truoc.embedded_chunks, truoc.chunks);
    assert!(db.count_keyword_matches("hà mã").unwrap() > 0);

    let bi_xoa = library
        .documents()
        .unwrap()
        .into_iter()
        .find(|d| d.title == "Ghi chép khác")
        .expect("phải tìm được tài liệu thứ hai");
    let doan_cua_no = bi_xoa.chunks;
    let ban_sao = bi_xoa.path.clone();
    assert!(ban_sao.exists());

    library.remove(&bi_xoa.id).unwrap();

    let sau = db.counts().unwrap();
    assert_eq!(sau.documents, truoc.documents - 1);
    assert_eq!(sau.chunks, truoc.chunks - doan_cua_no, "đoạn phải đi theo");
    assert_eq!(
        sau.embedded_chunks,
        truoc.embedded_chunks - doan_cua_no,
        "vector phải đi theo"
    );
    assert_eq!(
        db.count_keyword_matches("hà mã").unwrap(),
        0,
        "hàng FTS mồ côi vẫn trả về kết quả — đây là chỗ cascade lừa người viết"
    );
    // Và chỉ mục phải còn khớp với bảng nội dung, không chỉ "trông có vẻ đúng".
    db.fts_integrity()
        .expect("chỉ mục FTS lệch khỏi bảng chunks");

    // Bản sao tệp cũng đi theo: để lại thì tài liệu vừa xoá sống lại ở lần dựng lại kế tiếp.
    assert!(
        !ban_sao.exists(),
        "bản sao còn ở lại: {}",
        ban_sao.display()
    );

    // Tài liệu còn lại vẫn tìm được — xoá không được làm hỏng phần còn lại.
    assert!(!library.search("chứng chỉ", 5).await.unwrap().is_empty());
    // Và không kết quả nào còn trỏ về tài liệu đã xoá. Khẳng định theo **mã tài liệu**
    // chứ không phải "tìm `hà mã` ra rỗng": khi lượt AND không khớp gì, `search_keyword`
    // cố ý lùi về lượt OR, nên một từ lẻ như `mã` vẫn khớp tài liệu còn lại — đó là hành
    // vi đã thiết kế, không phải sót của việc xoá.
    let con_sot = library.search("hà mã", 5).await.unwrap();
    assert!(
        con_sot.iter().all(|h| h.document_id != bi_xoa.id),
        "kết quả còn trỏ về tài liệu đã xoá: {con_sot:#?}"
    );
}

// ---------------------------------------------------------------------------------
// 8. Ba tool trong một sổ đăng ký thật
// ---------------------------------------------------------------------------------

/// Đường vào thật của sản phẩm là plugin, không phải `Library::open`. Bài này giữ cho nó
/// chạy được: dựng cây, cắm thư viện, thấy đúng ba tool, và **gọi được** chúng.
#[tokio::test]
async fn plugin_cam_ba_tool_va_mot_seam() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    // Nạp trước rồi thả thư viện: không tool nào ở đây nạp tài liệu, và đó là có chủ ý —
    // một tài liệu không đáng tin không được phép bảo mô hình nạp thêm tài liệu khác.
    {
        let library = Library::open(kho.path(), None).unwrap();
        nap(&library, vec![path]).await;
    }

    let ctx = Context::root();
    ToolsPlugin.apply(&ctx.plugin("tools")).await.unwrap();
    RagPlugin::new(kho.path().to_path_buf(), None)
        .apply(&ctx.plugin("rag"))
        .await
        .expect("cắm được thư viện tài liệu");

    let tools = ctx.require::<Tools>().unwrap();
    let schemas = tools.schemas(None);
    let names: Vec<String> = schemas.iter().map(|s| s.name.to_string()).collect();
    for mong_doi in ["docs.search", "docs.read", "docs.list"] {
        assert!(names.iter().any(|n| n == mong_doi), "{names:?}");
    }

    // Ranh giới tin cậy: sổ đăng ký phải tự chèn lời cảnh báo vào mô tả của cả ba, vì mô
    // tả tool là thứ duy nhất mô hình đọc đúng lúc nó quyết định làm gì với văn bản.
    for schema in schemas
        .iter()
        .filter(|s| s.name.as_str().starts_with("docs."))
    {
        assert!(
            schema.description.contains(UNTRUSTED_NOTICE),
            "`{}` thiếu khung nội dung không đáng tin",
            schema.name
        );
    }

    // Seam phải dùng được từ ngoài, không chỉ từ ba tool bên trong.
    let docs = ctx.require::<Docs>().unwrap();
    assert_eq!(docs.documents().unwrap().len(), 1);

    // Và gọi thật, qua đúng đường mà mô hình đi: tên dạng wire, rồi `resolve`.
    let goi = async |ten: &str, args: serde_json::Value| -> String {
        let Resolution::Found(tool, name) = tools.resolve(None, ten) else {
            panic!("không tra được `{ten}`");
        };
        let arguments = args.as_object().cloned().unwrap_or_default();
        let ket_qua = tool
            .execute(&Invocation::new(name, "goi-1", arguments))
            .await
            .expect("tool phải chạy được");
        assert!(!ket_qua.is_error, "{ket_qua:?}");
        ket_qua.content
    };

    let danh_sach = goi("docs__list", serde_json::json!({})).await;
    assert!(danh_sach.contains("Sổ tay bảo mật nội bộ"), "{danh_sach}");
    // Không có bộ nhúng thì `docs.list` phải nói ra, để mô hình hỏi bằng từ khoá cụ thể.
    assert!(danh_sach.contains("từ khoá"), "{danh_sach}");

    let tim = goi(
        "docs__search",
        serde_json::json!({ "query": "chứng chỉ", "limit": 3 }),
    )
    .await;
    assert!(tim.contains("chứng chỉ"), "{tim}");
    // Trích dẫn được: tên tài liệu và số thứ tự đoạn nằm ngay trong văn bản trả về.
    assert!(tim.contains("[Sổ tay bảo mật nội bộ #"), "{tim}");

    let id = docs.documents().unwrap()[0].id.clone();
    let doc = goi(
        "docs__read",
        serde_json::json!({ "document_id": id, "offset": 0, "limit": 2 }),
    )
    .await;
    assert!(doc.contains("Sổ tay bảo mật nội bộ"), "{doc}");

    // Cả ba đều chỉ-đọc: việc nạp là một cú kéo thả của con người.
    for schema in schemas
        .iter()
        .filter(|s| s.name.as_str().starts_with("docs."))
    {
        let Resolution::Found(tool, _) = tools.resolve(None, schema.name.as_str()) else {
            panic!("{}", schema.name);
        };
        assert!(
            !tool.meta().mutating,
            "`{}` không được là mutating",
            schema.name
        );
        assert!(tool.meta().returns_untrusted_content, "{}", schema.name);
    }
}

/// Seam phải đủ để phía `app/` làm **mọi** việc của màn hình thư viện qua đúng một
/// handle: nạp, liệt kê, tìm, xoá.
///
/// Thiếu `ingest`/`remove` trên trait thì tầng trên chỉ cầm được `Arc<dyn DocLibrary>` và
/// buộc phải mở một `Library` thứ hai trên cùng thư mục. Hai handle với **hai bộ nhúng
/// khác nhau** sẽ ghi vector của hai không gian khác nhau vào cùng một bảng, và cosine
/// giữa chúng là một con số vô nghĩa trông y hệt một con số có nghĩa.
#[tokio::test]
async fn seam_du_de_nap_va_xoa_qua_mot_handle() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let embedder: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
    let docs: Arc<dyn DocLibrary> = Arc::new(Library::open(kho.path(), Some(embedder)).unwrap());

    // Nạp qua trait object — đây là đường mà `app/` sẽ đi.
    let su_kien: Vec<_> = docs.ingest(vec![path]).collect().await;
    assert!(su_kien.iter().any(|e| e.stage == IngestStage::Stored));
    assert!(su_kien.last().unwrap().finished);

    let tai_lieu = docs.documents().unwrap();
    assert_eq!(tai_lieu.len(), 1);
    assert!(!docs.search("chứng chỉ", 3).await.unwrap().is_empty());
    assert!(!docs.chunks(&tai_lieu[0].id, 0, 2).unwrap().is_empty());

    docs.remove(&tai_lieu[0].id).unwrap();
    assert!(docs.documents().unwrap().is_empty());
    assert_eq!(docs.stats().unwrap().chunks, 0);
}

// ---------------------------------------------------------------------------------
// 9. Đổi mô hình nhúng
// ---------------------------------------------------------------------------------

/// Một bộ nhúng giả thứ hai: **id khác, số chiều khác**, và vector nằm ở một không gian
/// hoàn toàn khác. Đây là bản mô phỏng của việc người dùng đổi provider từ Ollama sang
/// OpenAI — `nomic-embed-text` 768 chiều thành `text-embedding-3-small` 1536 chiều.
struct KhongGianKhac;

#[async_trait]
impl Embedder for KhongGianKhac {
    fn id(&self) -> &str {
        "khong-gian-khac"
    }

    fn dim(&self) -> Option<usize> {
        Some(4)
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, RagError> {
        // Tất định, và cố ý **không** cùng số chiều với `TuiTuGia`: trộn hai loại vector
        // này trong một bảng là đúng cái mà bài kiểm chứng bên dưới cấm.
        Ok(texts
            .iter()
            .map(|text| {
                let mut v = vec![0.0f32; 4];
                for (i, ch) in text.chars().enumerate() {
                    v[i % 4] += (ch as u32 % 7) as f32;
                }
                v
            })
            .collect())
    }

    async fn health(&self) -> bool {
        true
    }
}

/// Đổi mô hình nhúng phải xoá sạch vector — và **chỉ** vector.
///
/// Cosine giữa hai không gian nhúng khác nhau vẫn trả về một số trong `[-1, 1]`, vẫn xếp
/// hạng được, vẫn hiện lên giao diện. Không có gì báo lỗi. Đây là chỗ duy nhất trong cả
/// crate mà `semantic_ready` có thể nói dối, nên nó có một bài riêng.
#[tokio::test]
async fn doi_mo_hinh_nhung_xoa_vector_nhung_giu_tai_lieu() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let mot = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);
    let hai = viet(nguon.path(), "meo.txt", "Con mèo nằm trên chiếu.");

    // --- Vòng một: bộ nhúng A ---
    let (tai_lieu_truoc, doan_truoc) = {
        let a: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
        let library = Library::open(kho.path(), Some(a)).unwrap();
        nap(&library, vec![mot, hai]).await;

        let stats = library.stats().unwrap();
        assert!(stats.semantic_ready, "{:?}", stats.reason);
        assert_eq!(stats.embedded_chunks, stats.chunks);
        assert!(stats.chunks > 1);
        (stats.documents, stats.chunks)
    };

    // --- Vòng hai: mở lại cùng thư mục với bộ nhúng B ---
    let b: Arc<dyn Embedder> = Arc::new(KhongGianKhac);
    let library = Library::open(kho.path(), Some(b)).unwrap();

    let stats = library.stats().unwrap();
    // Vector cũ phải đi hết — không được còn một hàng nào của không gian cũ ở lại.
    assert_eq!(stats.embedded_chunks, 0, "vector của mô hình cũ còn ở lại");
    // Nhưng tài liệu và đoạn thì không được đụng tới.
    assert_eq!(stats.documents, tai_lieu_truoc, "tài liệu bị mất");
    assert_eq!(stats.chunks, doan_truoc, "đoạn bị mất");
    assert_eq!(library.documents().unwrap().len(), tai_lieu_truoc as usize);

    // Bất biến trung tâm của crate vẫn đứng: FTS5 chạy ngay trong lúc nhúng lại.
    let hits = library.search("chứng chỉ", 5).await.unwrap();
    assert!(
        !hits.is_empty(),
        "mất luôn tìm theo từ khoá khi đổi mô hình"
    );
    assert!(hits.iter().all(|h| h.matched_by == MatchedBy::Keyword));

    // Và `stats()` phải nói ra vì sao, gọi tên cả mô hình cũ lẫn mô hình mới. Người dùng
    // thấy số đoạn đã nhúng tụt về 0 mà không có lời giải thích sẽ tưởng thư viện hỏng.
    assert!(!stats.semantic_ready);
    assert_eq!(stats.embedder.as_deref(), Some("khong-gian-khac"));
    let reason = stats.reason.expect("phải nói ra vì sao");
    assert!(reason.contains("đổi"), "{reason}");
    assert!(
        reason.contains("tui-tu-gia"),
        "phải gọi tên mô hình cũ: {reason}"
    );
    assert!(reason.contains("khong-gian-khac"), "{reason}");
    assert!(
        reason.contains("Không có tài liệu nào bị mất"),
        "phải trấn an rằng tài liệu còn nguyên: {reason}"
    );

    // --- Nhúng lại bằng không gian của B ---
    let da_nhung = library.embed_pending().await.unwrap();
    assert_eq!(da_nhung, doan_truoc as usize);

    let sau = library.stats().unwrap();
    assert_eq!(sau.embedded_chunks, sau.chunks, "nhúng lại không đầy");
    assert_eq!(sau.documents, tai_lieu_truoc);
    assert_eq!(sau.chunks, doan_truoc);
    // Xong thì lời giải thích phải biến mất, không đọng lại mãi mãi.
    assert!(sau.semantic_ready, "{:?}", sau.reason);
    assert!(sau.reason.is_none(), "{:?}", sau.reason);

    // Mọi vector trong kho giờ thuộc đúng một không gian — của B, bốn chiều.
    let db = Store::open(
        &kho.path().join("library.sqlite"),
        &kho.path().join("files"),
    )
    .unwrap();
    let chieu: Vec<usize> = db
        .all_vectors()
        .unwrap()
        .iter()
        .map(|(_, v)| v.len())
        .collect();
    assert!(!chieu.is_empty());
    assert!(
        chieu.iter().all(|d| *d == 4),
        "vector lẫn hai không gian: {chieu:?}"
    );
}

/// Mở lại bằng **đúng** bộ nhúng cũ không được xoá gì cả — nếu không thì mỗi lần khởi
/// động ứng dụng là một lần nhúng lại cả thư viện.
#[tokio::test]
async fn mo_lai_cung_bo_nhung_khong_xoa_vector() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let truoc = {
        let a: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
        let library = Library::open(kho.path(), Some(a)).unwrap();
        nap(&library, vec![path]).await;
        library.stats().unwrap()
    };
    assert!(truoc.embedded_chunks > 0);

    let a: Arc<dyn Embedder> = Arc::new(TuiTuGia::moi());
    let library = Library::open(kho.path(), Some(a)).unwrap();
    let sau = library.stats().unwrap();
    assert_eq!(sau.embedded_chunks, truoc.embedded_chunks, "xoá vector oan");
    assert!(sau.semantic_ready);
    assert!(sau.reason.is_none());

    // Và gỡ hẳn bộ nhúng ra cũng không được xoá: vector cũ không sai, chúng chỉ tạm thời
    // không dùng tới. Bắt nhúng lại cả thư viện vì người dùng tắt Ollama một lát là sai.
    drop(library);
    let library = Library::open(kho.path(), None).unwrap();
    let db = Store::open(
        &kho.path().join("library.sqlite"),
        &kho.path().join("files"),
    )
    .unwrap();
    assert_eq!(db.counts().unwrap().embedded_chunks, truoc.embedded_chunks);
    // Không có bộ nhúng thì lý do nói về chuyện chưa cấu hình, không nói về chuyện đổi.
    let reason = library.stats().unwrap().reason.unwrap();
    assert!(reason.contains("Chưa cấu hình"), "{reason}");
}

// ---------------------------------------------------------------------------------
// 10. Lệch schema
// ---------------------------------------------------------------------------------

/// Hạ `user_version` xuống bản cũ, đúng như một kho do bản trước ghi ra.
fn ha_ban_schema(db: &Path, ban: i32) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.pragma_update(None, "user_version", ban).unwrap();
}

/// Lệch schema thì dựng lại **từ bản sao tệp trong kho**, không phải từ chỗ người dùng
/// lấy tệp — thư mục nguồn có thể đã bị dọn từ lâu.
#[tokio::test]
async fn lech_schema_thi_dung_lai_tu_ban_sao_trong_kho() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    let truoc = {
        let library = Library::open(kho.path(), None).unwrap();
        nap(&library, vec![path.clone()]).await;
        library.stats().unwrap()
    };
    assert!(truoc.chunks > 1);

    // Thư mục nguồn biến mất — đây chính là tình huống mà bản sao trong kho sinh ra để
    // chịu được.
    drop(nguon);
    assert!(!path.exists());

    ha_ban_schema(&kho.path().join("library.sqlite"), 1);

    let library = Library::open(kho.path(), None).unwrap();
    let sau = library.stats().unwrap();
    assert_eq!(
        sau.documents, truoc.documents,
        "tài liệu không dựng lại được"
    );
    assert_eq!(sau.chunks, truoc.chunks, "số đoạn lệch sau khi dựng lại");
    // Và chỉ mục FTS phải sống lại cùng, không chỉ các hàng trong bảng.
    assert!(!library.search("chứng chỉ", 5).await.unwrap().is_empty());
}

/// Nhưng khi bản sao **không** còn trong kho thì bảng `documents` là bản ghi duy nhất còn
/// lại về việc người dùng từng nạp những gì. Xoá nó đi để "dựng lại" là xoá đúng cái
/// không dựng lại được, nên ở đây phải từ chối mở và nói ra lý do.
#[tokio::test]
async fn lech_schema_ma_kho_tep_trong_thi_tu_choi_chu_khong_xoa() {
    let kho = TempDir::new().unwrap();
    let nguon = TempDir::new().unwrap();
    let path = viet(nguon.path(), "so-tay.md", VAN_BAN_TIENG_VIET);

    {
        let library = Library::open(kho.path(), None).unwrap();
        nap(&library, vec![path]).await;
    }

    // Ai đó dọn tay thư mục bản sao.
    for entry in std::fs::read_dir(kho.path().join("files"))
        .unwrap()
        .flatten()
    {
        std::fs::remove_file(entry.path()).unwrap();
    }
    ha_ban_schema(&kho.path().join("library.sqlite"), 1);

    let Err(err) = Library::open(kho.path(), None) else {
        panic!("phải từ chối mở khi kho tệp trống");
    };
    let noi_dung = err.to_string();
    assert!(noi_dung.contains("dựng lại được"), "{noi_dung}");
    // Lời từ chối phải nói cả bản schema lẫn đường thoát, nếu không nó chỉ là một lời từ
    // chối mà người dùng không làm gì được với nó.
    assert!(noi_dung.contains("chép tệp về"), "{noi_dung}");

    // Và dữ liệu vẫn còn nguyên trên đĩa — từ chối mở không được phép là xoá.
    let db = Store::open(&kho.path().join("library.sqlite"), kho.path()).unwrap();
    assert_eq!(
        db.counts().unwrap().documents,
        0,
        "kho phải được dựng lại sạch"
    );
}
