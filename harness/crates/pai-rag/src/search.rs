//! Hợp nhất hai bảng xếp hạng.
//!
//! # Vì sao là RRF chứ không phải cộng điểm
//!
//! BM25 của FTS5 và cosine giữa hai vector **không cùng thang đo**. BM25 là một số âm
//! không chặn dưới, phụ thuộc độ dài tài liệu và tần suất từ trong cả kho; cosine nằm gọn
//! trong `[-1, 1]` và với hầu hết bộ nhúng hiện đại thì mọi cặp văn bản tiếng Việt bất kỳ
//! đã rơi vào khoảng `0.6–0.9`. Cộng thẳng hai con số đó — hay chuẩn hoá rồi cộng — cho
//! ra một trọng số ngầm mà không ai chọn: tuỳ kho tài liệu, một bên áp đảo bên kia, và nó
//! đổi khi người dùng nạp thêm tệp.
//!
//! Reciprocal Rank Fusion chỉ nhìn **thứ hạng**, nên nó miễn nhiễm với chuyện đó:
//! `score = Σ 1/(k + rank)`. Một đoạn đứng nhất ở một bảng và vắng mặt ở bảng kia được
//! `1/61`; một đoạn đứng thứ ba ở cả hai bảng được `1/63 + 1/63`, và nó thắng — đúng ý:
//! đồng thuận giữa hai cách tìm là bằng chứng mạnh hơn một lần đứng nhất ở một cách.
//!
//! # Vì sao cosine chạy trong Rust
//!
//! Không có `sqlite-vec` ở đây. Mỗi lần tìm ngữ nghĩa nạp toàn bộ bảng `vectors` lên rồi
//! quét tuyến tính: **O(số đoạn × số chiều)**. Với 768 chiều, 10.000 đoạn là ~7,7 triệu
//! phép nhân-cộng, tức là vài mili-giây — không đáng bàn. Chỗ bắt đầu thấy chậm là quanh
//! **100.000 đoạn** (~300 MB vector đọc từ đĩa mỗi lần hỏi, và độ trễ nhảy lên hàng trăm
//! mili-giây); đó cũng là quy mô mà một thư viện cá nhân gần như không chạm tới. Khi nó
//! chạm tới, câu trả lời là một chỉ mục ANN, không phải một vòng lặp nhanh hơn.

use serde::{Deserialize, Serialize};

/// Hằng số của RRF. 60 là giá trị trong bài gốc của Cormack và cộng sự, và nó không phải
/// một tham số để chỉnh: nó làm phẳng chênh lệch giữa hạng 1 và hạng 2 để một bảng xếp
/// hạng tự tin không nuốt trọn kết quả.
pub const RRF_K: f32 = 60.0;

/// Vì sao một đoạn có mặt trong kết quả. Chuỗi serde khớp `DocumentHit.matched_by`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Keyword,
    Semantic,
    Both,
}

impl MatchedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchedBy::Keyword => "keyword",
            MatchedBy::Semantic => "semantic",
            MatchedBy::Both => "both",
        }
    }
}

/// Một đoạn sau khi hợp nhất.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ranked {
    pub chunk_id: i64,
    pub score: f32,
    pub matched_by: MatchedBy,
}

/// Hợp nhất hai danh sách **đã xếp hạng** (tốt nhất trước) thành một.
pub fn fuse(keyword: &[i64], semantic: &[i64], limit: usize) -> Vec<Ranked> {
    let mut merged: Vec<Ranked> = Vec::new();
    let mut seen: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

    let mut contribute = |ids: &[i64], from: MatchedBy, merged: &mut Vec<Ranked>| {
        for (index, id) in ids.iter().enumerate() {
            // Hạng đếm từ 1: hạng 0 làm mẫu số bằng `k` cho phần tử đầu tiên của cả hai
            // bảng và xoá mất chênh lệch giữa nó với phần tử thứ hai.
            let contribution = 1.0 / (RRF_K + (index + 1) as f32);
            match seen.get(id) {
                Some(at) => {
                    let row: &mut Ranked = &mut merged[*at];
                    row.score += contribution;
                    if row.matched_by != from {
                        row.matched_by = MatchedBy::Both;
                    }
                }
                None => {
                    seen.insert(*id, merged.len());
                    merged.push(Ranked {
                        chunk_id: *id,
                        score: contribution,
                        matched_by: from,
                    });
                }
            }
        }
    };

    contribute(keyword, MatchedBy::Keyword, &mut merged);
    contribute(semantic, MatchedBy::Semantic, &mut merged);

    // `total_cmp` chứ không phải `partial_cmp().unwrap()`: điểm là `f32` và một `NaN` lọt
    // vào đây sẽ làm cả lần sắp xếp hoảng loạn. Điểm RRF không thể là `NaN`, nhưng "không
    // thể" là thứ đúng cho tới lần đầu tiên nó sai.
    merged.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.chunk_id.cmp(&b.chunk_id))
    });
    merged.truncate(limit);
    merged
}

/// Cosine giữa hai vector. `0.0` khi lệch số chiều hoặc khi một bên là vector không —
/// cả hai đều là "không so sánh được", và một giá trị trung tính giữ nó ra khỏi kết quả
/// thay vì đẩy nó lên đầu.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Xếp hạng đoạn theo cosine với vector câu hỏi. Trả về mã đoạn, tốt nhất trước.
pub fn rank_by_cosine(query: &[f32], vectors: &[(i64, Vec<f32>)], limit: usize) -> Vec<i64> {
    let mut scored: Vec<(i64, f32)> = vectors
        .iter()
        .map(|(id, vector)| (*id, cosine(query, vector)))
        // Cosine bằng 0 nghĩa là lệch chiều hoặc trực giao hoàn toàn; giữ lại chỉ làm dài
        // danh sách mà không thêm thông tin nào cho phép hợp nhất.
        .filter(|(_, score)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(limit);
    scored.into_iter().map(|(id, _)| id).collect()
}
