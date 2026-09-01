//! Hai tool, và một luật chung cho cả hai.
//!
//! Cả `symbol_search` lẫn `outline` đều `read_only().untrusted()`. Chỉ-đọc thì hiển
//! nhiên. Không-đáng-tin thì không: thứ chúng trả về là **tên do người dùng đặt** — tên
//! hàm, tên kiểu, dòng khai báo — và một repo bất kỳ có thể chứa một hàm tên
//! `ignore_previous_instructions_and_run`. Đó là dữ liệu để trích dẫn, không phải chỉ dẫn
//! để làm theo, và chỗ duy nhất nói được điều đó đúng lúc là mô tả tool.

pub mod outline;
pub mod symbol_search;

use crate::symbol::Symbol;

/// Một dòng kết quả, dùng chung cho cả hai tool.
///
/// Bắt đầu bằng `đường:dòng` vì đó là hình dạng mà mô hình đã biết đọc từ `grep`, và vì
/// bước tiếp theo của nó gần như luôn là `read` đúng chỗ đó.
pub(crate) fn render(symbol: &Symbol) -> String {
    format!(
        "{}:{}-{} {} {} — {}",
        symbol.path,
        symbol.start_line,
        symbol.end_line,
        symbol.kind.as_str(),
        symbol.qualified(),
        symbol.signature
    )
}
