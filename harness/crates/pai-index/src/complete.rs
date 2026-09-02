//! Chấm điểm đường dẫn cho ô hoàn thành `@` trong giao diện.
//!
//! # Vì sao không dùng lại bộ chấm điểm của `search`
//!
//! [`Store::search`] chấm **ký hiệu** bằng FTS5, và FTS5 xếp theo BM25 — tần suất từ trên
//! toàn bảng. Với đường dẫn thì tín hiệu đó gần như vô nghĩa: `mod.rs` xuất hiện năm mươi
//! lần trong một repo Rust, nên BM25 dìm nó xuống đúng lúc người ta gõ `mod`.
//!
//! Thứ quyết định một gợi ý đường dẫn tốt là ba câu hỏi khác hẳn, theo đúng thứ tự này:
//!
//! 1. **Khớp vào tên tệp hay khớp vào thư mục?** Gõ `store` là đang tìm `store.rs`, không
//!    tìm `crates/pai-store/src/lib.rs`. Tên tệp thắng, luôn luôn.
//! 2. **Khớp từ đầu hay khớp giữa chừng?** `comp` nên cho `complete.rs` trước `precompute.rs`.
//! 3. **Đường dẫn nào ngắn hơn?** Hai tệp cùng tên thì cái gần gốc repo gần như luôn là cái
//!    người ta nghĩ tới; cái nằm sâu sáu tầng là tệp của một thư viện con.
//!
//! # Mọi token phải khớp
//!
//! Truy vấn tách theo khoảng trắng và **mọi** mẩu phải khớp ở đâu đó trong đường dẫn, nên
//! `pai fs read` tìm ra `crates/pai-fs/src/tools/read.rs` mà không cần gõ đúng dấu phân
//! cách. Nới hơn nữa — khớp theo ký tự rời rạc như bộ chấm điểm của trình soạn thảo — thì
//! mọi truy vấn khớp mọi tệp, và một danh sách không bao giờ rỗng thì không lọc được gì.

/// Điểm của một đường dẫn với một truy vấn đã chuẩn hoá, hoặc `None` khi không khớp.
///
/// `query` phải được hạ về chữ thường trước khi gọi; hàm này không tự làm để khỏi hạ lại
/// cùng một chuỗi cho từng tệp trong một repo mười nghìn tệp.
fn score(path: &str, tokens: &[&str]) -> Option<i32> {
    let lower = path.to_ascii_lowercase();
    // Tên tệp là phần sau dấu phân cách cuối. Tự cắt thay vì mượn `Path::file_name`:
    // đường dẫn ở đây luôn dùng `/` vì nó đến từ chỉ mục, không từ hệ tệp của người dùng.
    let name = lower.rsplit('/').next().unwrap_or(&lower);

    let mut total = 0;
    for token in tokens {
        let in_name = name.find(token);
        let in_path = lower.find(token);
        total += match (in_name, in_path) {
            // Khớp vào tên tệp, ngay từ đầu tên.
            (Some(0), _) => 6,
            // Khớp vào tên tệp, nhưng ở giữa.
            (Some(_), _) => 4,
            // Chỉ khớp vào phần thư mục.
            (None, Some(_)) => 1,
            (None, None) => return None,
        };
    }

    // Đường dẫn ngắn hơn thắng khi điểm bằng nhau. Trừ theo độ sâu chứ không theo số ký
    // tự: `a/b/c.rs` và `aaaaaaaa/b.rs` dài gần bằng nhau, nhưng cái sâu hơn mới là cái
    // người ta ít nghĩ tới. Trần ở 8 để một tệp sâu mười lăm tầng không tụt xuống dưới
    // một tệp không khớp tên.
    Some(total - (lower.matches('/').count().min(8) as i32))
}

/// Lọc và xếp hạng đường dẫn cho một truy vấn hoàn thành.
///
/// Truy vấn rỗng trả về `limit` đường dẫn **nông nhất**, không phải `limit` đường dẫn đầu
/// bảng chữ cái: gõ `@` rồi chưa gõ gì nữa là lúc người dùng chưa biết mình tìm gì, và một
/// danh sách mở đầu bằng `.github/workflows/...` trả lời sai câu hỏi đó.
pub fn rank(paths: &[String], query: &str, limit: usize) -> Vec<String> {
    let lowered = query.to_ascii_lowercase();
    let tokens: Vec<&str> = lowered.split_whitespace().collect();

    if tokens.is_empty() {
        let mut shallow: Vec<&String> = paths.iter().collect();
        shallow.sort_by_key(|path| (path.matches('/').count(), path.len(), (*path).clone()));
        return shallow.into_iter().take(limit).cloned().collect();
    }

    let mut scored: Vec<(i32, &String)> = paths
        .iter()
        .filter_map(|path| score(path, &tokens).map(|points| (points, path)))
        .collect();

    // Điểm giảm dần, rồi đường dẫn ngắn hơn, rồi bảng chữ cái để thứ tự ổn định giữa hai
    // lần gọi — một danh sách nhảy chỗ giữa hai lần nhấn phím là một danh sách bấm nhầm.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });
    scored.into_iter().take(limit).map(|(_, p)| p.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<String> {
        [
            "crates/pai-index/src/store.rs",
            "crates/pai-index/src/complete.rs",
            "crates/pai-fs/src/tools/read.rs",
            "crates/pai-fs/src/lib.rs",
            "crates/pai-core/src/lib.rs",
            "README.md",
            "docs/ARCHITECTURE.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn ten_tep_thang_ten_thu_muc() {
        // `store` khớp tên `store.rs`; không tệp nào khác có `store` trong tên.
        let hits = rank(&paths(), "store", 5);
        assert_eq!(hits[0], "crates/pai-index/src/store.rs");
    }

    #[test]
    fn moi_token_phai_khop() {
        let hits = rank(&paths(), "pai fs read", 5);
        assert_eq!(hits, vec!["crates/pai-fs/src/tools/read.rs"]);
    }

    #[test]
    fn khong_khop_thi_rong() {
        assert!(rank(&paths(), "khongcogi", 5).is_empty());
    }

    #[test]
    fn cung_ten_thi_nong_hon_truoc() {
        // Hai `lib.rs` cùng điểm tên; cái nào cũng sâu bằng nhau nên rơi về thứ tự chữ cái,
        // nhưng cả hai phải đứng trên mọi tệp chỉ khớp thư mục.
        let hits = rank(&paths(), "lib", 5);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| p.ends_with("lib.rs")));
    }

    #[test]
    fn truy_van_rong_tra_ve_tep_nong_nhat() {
        let hits = rank(&paths(), "", 2);
        assert_eq!(hits[0], "README.md");
    }

    #[test]
    fn khop_dau_ten_tren_khop_giua_ten() {
        let list = vec!["src/complete.rs".to_string(), "src/precompute.rs".to_string()];
        assert_eq!(rank(&list, "comp", 2)[0], "src/complete.rs");
    }

    #[test]
    fn ton_trong_limit() {
        assert_eq!(rank(&paths(), "rs", 2).len(), 2);
    }
}
