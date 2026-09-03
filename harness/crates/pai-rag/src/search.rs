//! Vì sao một đoạn có mặt trong kết quả.
//!
//! # Chỗ này từng là gì
//!
//! Trước đây module này chứa cả phép hợp nhất Reciprocal Rank Fusion lẫn phép tính
//! cosine — nó là nửa xếp hạng của tầng RAG viết bằng Rust. Cả hai giờ nằm ở
//! `services/rag/src/pai_rag_service/retrieval/fusion.py`, cùng chỗ với bước xếp hạng lại
//! bằng cross-encoder mà chúng nạp dữ liệu vào.
//!
//! Còn lại đúng một kiểu, và nó ở lại vì [`crate::library::Hit`] mang nó: giao diện vẽ
//! một nhãn khác nhau cho đoạn khớp từ khoá, khớp ngữ nghĩa, và khớp cả hai — "cả hai" là
//! tín hiệu mạnh nhất mà người dùng đọc được mà không cần hiểu RRF là gì.

use serde::{Deserialize, Serialize};

/// Chuỗi serde khớp `DocumentHit.matched_by` phía `app/` và `matchedBy` bên Python.
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

    /// Từ chuỗi trên dây. Nhãn lạ rơi về [`MatchedBy::Keyword`] — nó là nhánh không cần
    /// bộ nhúng, nên đoán nhầm về phía đó không hứa với người dùng điều gì không có.
    pub fn parse(value: &str) -> MatchedBy {
        match value.trim().to_ascii_lowercase().as_str() {
            "semantic" => MatchedBy::Semantic,
            "both" => MatchedBy::Both,
            _ => MatchedBy::Keyword,
        }
    }
}
