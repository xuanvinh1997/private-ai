//! Đo thật cái trần mà `docs/ROADMAP.md` mới chỉ ước lượng.
//!
//! Dòng nợ ấy viết: "Cosine quét tuyến tính toàn bảng `vectors`. Ước lượng bắt đầu chậm
//! quanh 100.000 đoạn. Chưa benchmark ở quy mô đó."
//!
//! Một con số ước lượng không kiểm được thì không dùng để quyết định gì: nó vừa có thể
//! đang doạ người ta đi tối ưu một thứ đủ nhanh, vừa có thể đang che một chỗ đã chậm từ
//! lâu. Bài này biến nó thành một dữ kiện, và **khoá** dữ kiện ấy lại — nếu ai đó làm cho
//! đường này chậm đi mười lần, bài đỏ.
//!
//! Trần đặt rộng tay (500ms cho 100k) vì đây là bài chạy trên máy lập trình viên, không
//! phải một phòng đo. Nó bắt được thứ đáng bắt: một hồi quy về **bậc độ lớn**.

use std::time::Instant;

use pai_rag::search::rank_by_cosine;

/// Đủ giống một bảng thật: 100k đoạn, 768 chiều — đúng số chiều `embeddinggemma` trả về.
const N: usize = 100_000;
const DIM: usize = 768;

fn vectors() -> Vec<(i64, Vec<f32>)> {
    // Sinh tất định bằng một bộ sinh tuyến tính rẻ tiền: cần dữ liệu *thật* để đo, không
    // cần dữ liệu ngẫu nhiên tốt, và một hạt cố định làm số đo lặp lại được giữa hai lần.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16_777_216.0 - 0.5
    };
    (0..N)
        .map(|i| (i as i64, (0..DIM).map(|_| next()).collect()))
        .collect()
}

#[test]
fn cosine_tren_100k_doan_van_du_nhanh() {
    let table = vectors();
    let query: Vec<f32> = table[N / 2].1.clone();

    // Một lần chạy nguội trước để trang bộ nhớ đã được chạm tới; ta đo phép tính, không đo
    // lần chạm trang đầu tiên.
    let _ = rank_by_cosine(&query, &table[..1000], 10);

    let start = Instant::now();
    let hits = rank_by_cosine(&query, &table, 10);
    let elapsed = start.elapsed();

    assert_eq!(hits.len(), 10, "phải trả về đúng số kết quả đã xin");
    assert_eq!(
        hits[0],
        (N / 2) as i64,
        "chính vector truy vấn phải đứng đầu — sai thì phép cosine sai, không phải chậm"
    );

    eprintln!("cosine {N} đoạn × {DIM} chiều: {elapsed:?}");
    assert!(
        elapsed.as_millis() < 500,
        "quét tuyến tính {N} đoạn mất {elapsed:?}, quá 500ms — đây là lúc cần chỉ mục ANN"
    );
}
