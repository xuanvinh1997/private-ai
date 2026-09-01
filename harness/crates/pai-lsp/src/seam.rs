//! Seam của crate này, và từ vựng đi qua nó.
//!
//! Một seam duy nhất: "có ai trả lời được câu hỏi về ngữ nghĩa mã nguồn không". Bản cài
//! đặt trong crate này nuôi những tiến trình con trên máy ([`crate::pool::StdioServers`]),
//! nhưng hình dạng ở đây không nói gì về tiến trình con — nên trỏ nó vào một sandbox từ xa
//! sau này là thay một provider, không phải sửa tool.
//!
//! **Toạ độ ở biên này là 1-based theo ký tự**, giống hệt thứ `read` in ra và thứ con
//! người đọc. Việc đổi sang 0-based/UTF-16 của LSP nằm gọn trong provider, ở đúng một
//! chỗ — xem [`crate::pool`]. Để nó rò ra tới seam là mời mỗi consumer tự đổi một lần, và
//! lệch một cột thì câu trả lời không sai hẳn, nó chỉ trỏ vào ký hiệu bên cạnh.

use std::path::PathBuf;

use async_trait::async_trait;
use pai_core::ServiceKey;

use crate::error::LspError;

/// Bốn thao tác, và cả bốn đều là thứ tree-sitter **không** làm được.
///
/// Ranh giới với `pai-index` là cố ý và cần được giữ: `symbol_search` và `outline` trả lời
/// "cái tên này khai ở đâu" bằng cú pháp thuần tuý, chạy offline, không cần cài gì. Những
/// gì ở đây đòi một trình biên dịch: đi tới định nghĩa qua nhiều tệp và qua `use`, tìm mọi
/// nơi *tham chiếu* (không phải mọi nơi trùng chữ), kiểu suy ra được, và lỗi biên dịch
/// thật. Thêm một thao tác vào đây mà tree-sitter cũng làm được là thêm một tool thứ hai
/// cho cùng một câu hỏi, rồi mô hình phải đoán xem cái nào đúng.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Definition,
    References,
    Hover,
    Diagnostics,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Definition => "definition",
            Operation::References => "references",
            Operation::Hover => "hover",
            Operation::Diagnostics => "diagnostics",
        }
    }

    /// `diagnostics` nói về cả tệp, ba cái kia nói về một con trỏ. Phân biệt ở đây để chỗ
    /// kiểm tham số không phải chép lại danh sách.
    pub fn needs_position(self) -> bool {
        !matches!(self, Operation::Diagnostics)
    }
}

/// Một câu hỏi: thao tác nào, ở tệp nào, tại con trỏ nào (1-based).
#[derive(Clone, Debug)]
pub struct Query {
    pub op: Operation,
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
}

/// Một chỗ trong mã, đã quy về toạ độ mà con người và `read` cùng dùng.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Tương đối với thư mục làm việc khi nằm trong đó, tuyệt đối khi không.
    pub path: String,
    pub line: u32,
    pub column: u32,
    /// Dòng mã ở đó, đã cắt hai đầu. Rỗng khi tệp nằm ngoài thư mục làm việc: ranh giới
    /// của `pai-fs` không có ngoại lệ cho crate này.
    pub text: String,
    /// `read` với tới được không. Nói ra để mô hình không đi đọc một tệp nó sẽ bị từ chối.
    pub reachable: bool,
}

/// Một chẩn đoán của trình biên dịch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub line: u32,
    pub column: u32,
    pub severity: &'static str,
    pub source: Option<String>,
    pub message: String,
}

/// Câu trả lời, kèm một điều server nói về chính nó.
///
/// `busy` là "server còn đang lập chỉ mục". Nó đi kèm **mọi** dạng câu trả lời chứ không
/// chỉ dạng rỗng, vì một danh sách tham chiếu thu được giữa lúc đang nạp là một danh sách
/// *thiếu*, và mô hình cần biết nó thiếu chứ không phải nó đủ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Locations {
        hits: Vec<Hit>,
        truncated: bool,
        busy: bool,
    },
    Hover {
        text: String,
        busy: bool,
    },
    Diagnostics {
        notes: Vec<Note>,
        busy: bool,
    },
}

#[async_trait]
pub trait LanguageServers: Send + Sync + 'static {
    /// Ngôn ngữ nào **thật sự** có server trên máy này. Danh sách rỗng thì plugin đã không
    /// đăng ký tool nào — xem [`crate::plugin`].
    fn languages(&self) -> Vec<String>;

    async fn ask(&self, query: &Query) -> Result<Answer, LspError>;
}

/// Không có provider = không có tool `lsp`, và mọi thứ khác vẫn chạy.
pub enum Lsp {}
impl ServiceKey for Lsp {
    type Api = dyn LanguageServers;
    const NAME: &'static str = "lsp";
}
