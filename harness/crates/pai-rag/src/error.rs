//! Lỗi của thư viện tài liệu.
//!
//! Mỗi nhánh ở đây tồn tại vì **người dùng phải đọc được nó và biết làm gì tiếp**. Một
//! `RagError::Other(String)` duy nhất thì rẻ để viết và vô dụng để đọc: người vừa kéo hai
//! mươi tệp vào cần phân biệt "tệp này quá to" với "bộ nhúng đang tắt" — cái đầu họ sửa
//! bằng cách bỏ tệp ra, cái sau bằng cách bật Ollama lên.
//!
//! # Chỗ này ngắn đi vì lý do gì
//!
//! Bản trước có thêm `TooLarge`, `Binary`, `Empty`, `Unsupported` — những lỗi của việc
//! **đọc một tệp**, khi việc ấy còn nằm trong Rust. Giờ nó nằm ở `services/rag/`, và
//! phía đó đã dựng sẵn câu tiếng Việt nói rõ tệp nào hỏng vì sao. Dựng lại một cây lỗi
//! song song ở đây chỉ để phân loại một chuỗi đã hoàn chỉnh là thêm một chỗ để hai bên
//! nói khác nhau về cùng một sự việc.

/// Lỗi ở tầng thư viện tài liệu.
#[derive(Debug, thiserror::Error)]
pub enum RagError {
    /// Service `pai-rag-service` không chạy, không trả lời, hoặc trả về thứ không đọc
    /// được. Thông điệp đi kèm luôn nói ra việc phải làm — cài `uv`, dựng Docker, hoặc
    /// xem stderr.
    #[error("{0}")]
    Service(String),

    #[error("không có tài liệu nào mang mã `{0}`")]
    NotFound(String),

    /// Một khả năng có thật nhưng đang tắt: chưa chọn model nhúng, chưa mở dự án nào.
    #[error("{0}")]
    Unavailable(String),
}

