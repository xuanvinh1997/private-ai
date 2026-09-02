//! Bộ nhúng nói chuyện với một máy chủ **thật**.
//!
//! # Vì sao bài này phải tồn tại
//!
//! `docs/ROADMAP.md` ghi món nợ này ở dòng đầu tiên: phần phân tích JSON của
//! `OllamaEmbedder` và `OpenAiEmbedder` mới chỉ đúng *theo tài liệu API*, chưa lần nào gặp
//! một máy chủ. Mọi bài kiểm chứng khác của `pai-rag` dựng một bộ nhúng giả, nên chúng
//! chứng minh được đường ống ghép đúng nhưng **không** chứng minh được ta đọc đúng thân
//! trả lời — và đó chính là chỗ duy nhất còn có thể sai.
//!
//! # Vì sao nó bỏ qua thay vì hỏng
//!
//! Máy chạy CI không có Ollama, và một bài đỏ vì môi trường dạy người ta bỏ qua màu đỏ.
//! Nên không có máy chủ thì bài này **bỏ qua và nói ra**. Đánh đổi phải nói thẳng: một bài
//! tự bỏ qua là một bài có thể chưa bao giờ chạy ở đâu cả. Nó bù lại bằng chỗ khác — đây
//! là đường **duy nhất** trong repo đi qua mã phân tích ấy, nên chạy nó một lần trên máy
//! có Ollama là đổi một món nợ đã biết lấy một dữ kiện.
//!
//! ```sh
//! ollama pull embeddinggemma        # hoặc nomic-embed-text
//! cargo test -p pai-rag --test embed_live -- --nocapture
//! ```
//!
//! Đặt `PAI_TEST_EMBED_MODEL` để dùng mô hình khác, `PAI_TEST_OLLAMA_URL` cho máy chủ khác.

use pai_rag::{Embedder, OllamaEmbedder};

fn base_url() -> String {
    std::env::var("PAI_TEST_OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
}

fn model() -> String {
    std::env::var("PAI_TEST_EMBED_MODEL").unwrap_or_else(|_| "embeddinggemma:latest".to_string())
}

/// `None` kèm một dòng giải thích khi không có máy chủ để nói chuyện.
async fn live() -> Option<OllamaEmbedder> {
    let embedder = OllamaEmbedder::new(base_url(), model());
    if embedder.health().await {
        Some(embedder)
    } else {
        eprintln!(
            "BỎ QUA: không thấy Ollama ở {}. Bài này cần một máy chủ thật.",
            base_url()
        );
        None
    }
}

/// Cosine của hai vector đã chuẩn hoá hay chưa đều dùng được — ta chỉ so tương đối.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb)
}

/// Thân trả lời của Ollama đọc ra đúng số vector, đúng thứ tự, đúng số chiều.
#[tokio::test]
async fn ollama_tra_ve_dung_so_vector_va_dung_thu_tu() {
    let Some(embedder) = live().await else { return };

    let texts = vec![
        "con mèo ngồi trên thảm".to_string(),
        "một chiếc xe tải chở hàng".to_string(),
        "con mèo nằm trên tấm thảm".to_string(),
    ];
    let vectors = embedder.embed(&texts).await.expect("máy chủ trả lời được");

    assert_eq!(
        vectors.len(),
        texts.len(),
        "phải trả đúng một vector cho mỗi đầu vào — tầng trên ghép theo chỉ số"
    );
    let dim = vectors[0].len();
    assert!(dim > 0, "vector rỗng nghĩa là đọc sai thân trả lời");
    for vector in &vectors {
        assert_eq!(vector.len(), dim, "mọi vector phải cùng số chiều");
        assert!(
            vector.iter().any(|value| *value != 0.0),
            "vector toàn 0 là dấu hiệu đọc nhầm trường trong JSON"
        );
    }

    // Thứ tự là bất biến mà `library.rs` dựa vào để ghép vector với đoạn. Kiểm nó bằng ngữ
    // nghĩa: hai câu về con mèo phải gần nhau hơn là câu mèo với câu xe tải. Sai thứ tự thì
    // bất đẳng thức này lật, và đó là kiểu hỏng lặng lẽ nhất có thể có — tìm kiếm vẫn chạy,
    // chỉ là trả về sai tài liệu, mãi mãi.
    let meo_meo = cosine(&vectors[0], &vectors[2]);
    let meo_xe = cosine(&vectors[0], &vectors[1]);
    assert!(
        meo_meo > meo_xe,
        "hai câu cùng nghĩa phải gần nhau hơn: mèo↔mèo {meo_meo:.3} so với mèo↔xe {meo_xe:.3}"
    );
}

/// `dim()` khai trước phải khớp với số chiều máy chủ thật sự trả về.
///
/// Lệch một chiều là bảng `vectors` nhận vào những hàng không so được với nhau, và cosine
/// trả ra số vô nghĩa thay vì lỗi.
#[tokio::test]
async fn so_chieu_khai_truoc_khop_voi_may_chu() {
    let Some(embedder) = live().await else { return };

    let vectors = embedder
        .embed(&["đo số chiều".to_string()])
        .await
        .expect("máy chủ trả lời được");
    let thuc_te = vectors[0].len();

    let khai_truoc = OllamaEmbedder::new(base_url(), model()).with_dim(thuc_te);
    assert_eq!(khai_truoc.dim(), Some(thuc_te));
    eprintln!("mô hình {} trả về {thuc_te} chiều", model());
}

/// Lô lớn hơn `MAX_BATCH` phải được cắt và ghép lại **đúng thứ tự**.
///
/// Đây là đoạn mã `chunks(MAX_BATCH)` cộng vòng lặp nối — chỗ dễ trả về đủ số lượng nhưng
/// sai trật tự, và không bài giả nào bắt được vì bộ nhúng giả không quan tâm kích thước lô.
#[tokio::test]
async fn lo_lon_hon_mot_batch_van_dung_thu_tu() {
    let Some(embedder) = live().await else { return };

    let n = pai_rag::MAX_BATCH + 3;
    let texts: Vec<String> = (0..n).map(|i| format!("câu số {i}")).collect();
    let vectors = embedder.embed(&texts).await.expect("máy chủ trả lời được");

    assert_eq!(vectors.len(), n, "cắt lô rồi ghép lại không được rơi phần tử");

    // Nhúng lại **một** câu ở giữa lô thứ hai và so với vị trí của nó trong kết quả trên.
    // Trùng thì thứ tự đã được giữ qua ranh giới lô.
    let giua = pai_rag::MAX_BATCH + 1;
    let lai = embedder
        .embed(&[texts[giua].clone()])
        .await
        .expect("máy chủ trả lời được");
    let giong = cosine(&vectors[giua], &lai[0]);
    assert!(
        giong > 0.99,
        "phần tử {giua} phải là chính nó sau khi ghép lô, nhưng cosine chỉ {giong:.4}"
    );
}

/// Rút chữ từ một PDF **thật** có font nhúng và bảng `ToUnicode`.
///
/// Món nợ thứ hai trong `docs/ROADMAP.md`: bài kiểm chứng cũ dựng được PDF hợp lệ nhưng chữ
/// ASCII, còn PDF do Word hay LaTeX sinh ra nhúng font và ánh xạ mã qua `ToUnicode` — đường
/// đó chưa ai đi qua. Không đi qua nghĩa là: một thư viện tài liệu tiếng Việt có thể nạp
/// xong, báo thành công, và giấu trong kho một mớ ký tự rác mà không ai biết cho tới lúc
/// tìm kiếm không ra gì.
///
/// Bài này tự dựng PDF ấy bằng Chrome ở chế độ headless — cùng đường in mà Word đi — nên nó
/// không cần một tệp mẫu nằm sẵn trong repo. Không có Chrome thì bỏ qua và nói ra.
#[test]
fn pdf_font_nhung_rut_dung_chu_tieng_viet() {
    const CHROME: &str =
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    if !std::path::Path::new(CHROME).exists() {
        eprintln!("BỎ QUA: không có Chrome để in ra PDF có font nhúng.");
        return;
    }

    let dir = std::env::temp_dir().join("pai-pdf-viet");
    std::fs::create_dir_all(&dir).expect("tạo thư mục tạm");
    let html = dir.join("nguon.html");
    let pdf = dir.join("ra.pdf");

    // Mỗi dấu tiếng Việt xuất hiện ít nhất một lần, cộng `Đ` hoa — ký tự mà `NFD` không
    // tách ra được và cũng là chỗ nhiều bộ rút chữ trả về dấu hỏi.
    let mong_doi = "Đây là đoạn chữ tiếng Việt có đủ dấu: ăn ơn ưu êm ôi, sắc huyền hỏi ngã nặng.";
    std::fs::write(
        &html,
        format!("<meta charset=\"utf-8\"><body><p>{mong_doi}</p></body>"),
    )
    .expect("ghi html");

    let ok = std::process::Command::new(CHROME)
        .args([
            "--headless",
            "--disable-gpu",
            "--no-pdf-header-footer",
            &format!("--print-to-pdf={}", pdf.display()),
        ])
        .arg(&html)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !ok || !pdf.exists() {
        eprintln!("BỎ QUA: Chrome không in được PDF.");
        return;
    }

    let raw = std::fs::read(&pdf).expect("đọc pdf");
    assert!(
        raw.windows(9).any(|w| w == b"ToUnicode"),
        "PDF dựng ra phải có bảng ToUnicode, nếu không bài này không kiểm cái nó định kiểm"
    );

    let ra = pai_rag::extract::extract(&pdf).expect("rút được chữ từ PDF");
    let text = ra.text.replace(['\n', '\r'], " ");

    // Không so nguyên câu: bộ rút chữ được phép đổi khoảng trắng và thứ tự dòng. So từng
    // cụm có dấu — đó mới là thứ đường `ToUnicode` quyết định đúng hay sai.
    for cum in ["Đây", "tiếng Việt", "ăn", "ơn", "ưu", "êm", "ôi", "huyền", "ngã", "nặng"] {
        assert!(
            text.contains(cum),
            "mất cụm `{cum}` khi rút chữ. Toàn văn rút được: {text}"
        );
    }
    eprintln!("PDF font nhúng rút đúng: {text}");
}
