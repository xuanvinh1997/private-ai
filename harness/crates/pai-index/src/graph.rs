//! Từ vựng của đồ thị, và cái mà một cạnh **không** hứa.
//!
//! Chỉ mục ký hiệu trả lời "khai báo ở đâu". Đồ thị trả lời "cái gì nối với cái gì" — và
//! đó là câu hỏi mà một chỉ mục cú pháp thuần tuý chỉ trả lời được **gần đúng**.
//!
//! # Một cạnh là gì, thật sự
//!
//! Đúng một loại cạnh là sự thật cú pháp: [`EdgeKind::Contains`]. Cha chứa con suy ra từ
//! bao hàm phạm vi byte trong cùng một cây, nên nó đúng hay sai cùng lúc với việc tệp có
//! parse được hay không.
//!
//! Năm loại còn lại là **phỏng đoán theo tên**. Không có phân tích kiểu, không có phân
//! giải module, không có bảng ký hiệu của trình biên dịch — chỉ có một cái tên ở chỗ gọi
//! và một cái tên ở chỗ khai báo. `run()` trong tệp này nối tới ký hiệu `run` gần nhất mà
//! bảng biết, theo bậc ưu tiên ở [`crate::store::Store::rebuild_edges`]. Khi trong cùng
//! một bậc còn nhiều ứng viên thì **cả n cạnh được ghi**, chứ không chọn bừa một cái.
//!
//! Lý do ghi cả n thay vì bỏ: câu hỏi mà đồ thị này phục vụ là "ai gọi hàm này" trước một
//! lần sửa. Trả về ba ứng viên trong đó có cái đúng khiến mô hình đi đọc ba chỗ; trả về
//! rỗng khiến nó tin rằng **không ai gọi** và xoá hàm đi. Sai theo hướng thứ hai đắt hơn
//! nhiều. Nhưng "cả n" có trần — xem `MAX_CANDIDATES` — vì một cái tên có hai mươi khai
//! báo thì hai mươi cạnh không mang thông tin nào cả, chỉ mang nhiễu.
//!
//! Vì thế mọi kết quả tool đi ra mô hình đều mang [`NAME_BASED_NOTICE`]. Một đồ thị được
//! trình bày như sự thật trong khi nó là phỏng đoán sẽ khiến mô hình kết luận sai **và tự
//! tin**, đúng kiểu sai mà không ai bắt được. Cùng lý do khiến `pai-sandbox` báo
//! `Enforcement::Partial` thay vì làm tròn lên thành "có giam".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Câu phải xuất hiện trong mọi kết quả tool có kèm cạnh.
///
/// Nó nằm trong **nội dung trả về** chứ không chỉ trong mô tả tool, vì mô tả được đọc một
/// lần lúc liệt kê còn nội dung thì nằm ngay cạnh cái danh sách cạnh mà mô hình đang định
/// tin.
pub const NAME_BASED_NOTICE: &str = "Cạnh `calls`, `imports`, `implements`, `extends` và \
`references` là suy đoán theo tên, không phải phân tích kiểu: một tên trùng nhau ở nhiều \
nơi sinh ra nhiều cạnh, và một lời gọi qua biến hay qua trait object có thể không sinh \
cạnh nào. Chỉ `contains` là chắc chắn. Kiểm lại bằng `read` trước khi dựa vào nó để sửa mã.";

/// Nhãn `kind` của đỉnh đại diện cho **cả một tệp**.
///
/// Nó không phải một [`crate::SymbolKind`], và đó là cố ý: bảng bốn loại kia là thứ mô
/// hình lọc `symbol_search` bằng, thêm một loại vào đó là thêm một chỗ để đoán trượt. Đỉnh
/// module chỉ tồn tại trong đồ thị — nơi nó là chủ nhà cho `import` ở tầng tệp và là gốc
/// cho `contains` của những ký hiệu không có cha.
pub const MODULE_KIND: &str = "module";

/// Sáu quan hệ, đúng bằng hợp đồng wire. Không có loại thứ bảy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// Một lời gọi bên trong thân một ký hiệu.
    Calls,
    /// `use` / `import` / `require` / `from ... import`.
    Imports,
    /// Cha chứa con. Loại duy nhất không phải phỏng đoán.
    Contains,
    /// `impl Trait for T`, `class A implements I`.
    Implements,
    /// `class A extends B`, `class A(B)`.
    Extends,
    /// Tên một kiểu xuất hiện trong chữ ký: tham số, kiểu trả về, chú thích.
    References,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Contains => "contains",
            EdgeKind::Implements => "implements",
            EdgeKind::Extends => "extends",
            EdgeKind::References => "references",
        }
    }

    pub fn parse(text: &str) -> Option<EdgeKind> {
        match text {
            "calls" => Some(EdgeKind::Calls),
            "imports" => Some(EdgeKind::Imports),
            "contains" => Some(EdgeKind::Contains),
            "implements" => Some(EdgeKind::Implements),
            "extends" => Some(EdgeKind::Extends),
            "references" => Some(EdgeKind::References),
            _ => None,
        }
    }

    /// Cạnh này có phải sự thật cú pháp không. Xem ghi chú đầu tệp.
    pub fn is_structural(self) -> bool {
        matches!(self, EdgeKind::Contains)
    }

    /// Đỉnh module có được làm đích của loại cạnh này không.
    ///
    /// `import os` trỏ đúng vào một tệp; `os.path()` thì không — một lời gọi không bao giờ
    /// nhắm vào một tệp, nên cho phép nó khớp đỉnh module là tự sinh ra cạnh sai.
    pub fn may_target_module(self) -> bool {
        matches!(self, EdgeKind::Imports | EdgeKind::Contains)
    }
}

/// Một đỉnh. `kind` là chuỗi chứ không phải enum vì [`MODULE_KIND`] không nằm trong bảng
/// bốn loại ký hiệu, và vì phía giao diện nhận nó dưới dạng chuỗi.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct GraphEdge {
    pub src: i64,
    pub dst: i64,
    pub kind: EdgeKind,
}

/// Một lát cắt quanh một ký hiệu, đã cắt cho vừa màn hình và vừa ngữ cảnh.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Neighborhood {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Đã cắt: hoặc vì `depth` xin vượt trần, hoặc vì chạm trần số đỉnh/số cạnh. Nói ra
    /// chứ không im, nếu không "không còn cạnh nào nữa" trông y hệt "hết cạnh rồi".
    pub truncated: bool,
}

/// Một thư mục và những gì nó chứa. Đây là phần "module" của bản đồ kiến trúc.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DirectorySummary {
    pub path: String,
    pub files: u32,
    pub symbols: u32,
}

/// Một ký hiệu có nhiều cạnh nhất — chỗ đáng đọc đầu tiên trong một kho lạ.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CentralSymbol {
    pub node: GraphNode,
    pub incoming: u32,
    pub outgoing: u32,
}

impl CentralSymbol {
    pub fn degree(&self) -> u32 {
        self.incoming + self.outgoing
    }
}

/// Bản đồ kiến trúc: đọc trước khi đọc mã.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Overview {
    pub directories: Vec<DirectorySummary>,
    /// `(ngôn ngữ, số tệp)`, nhiều trước.
    pub languages: Vec<(String, u32)>,
    pub central: Vec<CentralSymbol>,
    /// Số thư mục đã bị cắt khỏi `directories`.
    pub directories_omitted: u32,
}

/// Tình trạng chỉ mục. Chuyển thẳng sang `IndexStats` của hợp đồng wire.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub files: u32,
    /// **Không** kể đỉnh module: con số này là "repo có bao nhiêu khai báo", và một đỉnh
    /// mỗi tệp cộng vào đó chỉ làm nó hết so sánh được với lần quét trước.
    pub symbols: u32,
    pub edges: u32,
    /// `(ngôn ngữ, số tệp)`, nhiều trước.
    pub languages: Vec<(String, u32)>,
    /// Lần quét gần nhất, epoch mili-giây.
    pub scanned_at: Option<i64>,
}

/// Chủ nhà của một tham chiếu: ký hiệu nào chứa chỗ nhắc tới nó.
///
/// Ba nhánh chứ không phải một chuỗi tên, vì độ chắc chắn của ba trường hợp khác nhau và
/// gộp lại thì cả ba tụt xuống mức thấp nhất.
#[derive(Clone, Debug, PartialEq)]
pub enum Owner {
    /// Chỉ số trong `Vec<Symbol>` vừa trích. Chính xác tuyệt đối.
    Symbol(usize),
    /// Một `@def.scope` — `impl Foo`, `mod bar`. Nó không tự mình là ký hiệu, nên phải tra
    /// tên **trong chính tệp này**; `impl Foo` tìm thấy `struct Foo` là trường hợp thường.
    Scope(String),
    /// Tầng tệp: `use` ở đầu tệp không nằm trong ký hiệu nào. Chủ nhà là đỉnh module.
    File,
}

/// Đích của một tham chiếu.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Chỉ số trong `Vec<Symbol>` vừa trích — dùng cho `contains`, thứ duy nhất biết chắc.
    Symbol(usize),
    /// Một cái tên, chờ phân giải. Đây là chỗ đồ thị thôi là sự thật và thành phỏng đoán.
    Name(String),
}

/// Một quan hệ vừa nhìn thấy trong cây cú pháp, **chưa** phân giải.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub from: Owner,
    pub to: Target,
    pub kind: EdgeKind,
    /// Dòng của chỗ nhắc tới, đánh số từ 1.
    pub line: u32,
}
