//! Bảng ngôn ngữ: thêm một server là thêm **một hàng**.
//!
//! Giao thức là chuẩn, nên [`crate::client`] không biết tên server nào cả. Cái duy nhất
//! khác nhau giữa `rust-analyzer` và `pyright` là dòng lệnh khởi động và mấy tuỳ chọn
//! khởi tạo, và đó chính xác là những trường dưới đây. Cùng ý với `pai-index::lang`: phần
//! khó của việc thêm một ngôn ngữ không phải là mã.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Chờ bắt tay bao lâu trước khi trả lời "chưa sẵn sàng".
///
/// `initialize` của `rust-analyzer` trả lời nhanh — việc nạp workspace mất hàng chục giây
/// diễn ra *sau* đó và được báo qua `$/progress`, không chặn cái bắt tay. Nên hai mươi
/// giây ở đây là để chờ một tiến trình khởi động, không phải để chờ nó lập chỉ mục xong;
/// dài hơn nữa thì lượt của người dùng đứng im vì một thứ không phải của họ.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// Hạn cho một truy vấn đã gửi đi. Sáu mươi giây, lấy từ cấu hình của dsh.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Chờ `publishDiagnostics` bao lâu sau khi mở tệp.
///
/// Chẩn đoán là **thông báo đẩy**, không phải câu trả lời cho một câu hỏi: không có gì để
/// mà hết giờ ngoài sự kiên nhẫn của ta. Năm giây đủ cho một tệp vừa mở trong một
/// workspace đã nạp xong; chưa nạp xong thì `busy` nói ra điều đó thay vì im lặng trả về
/// "không có lỗi nào".
pub const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(5);

/// Trần số vị trí trả về cho một lần hỏi. Lấy từ dsh.
///
/// Một `references` trên `String::new` trả về hàng nghìn chỗ, và cái mô hình cần là hai
/// mươi chỗ đầu cộng với việc biết rằng còn nữa — chứ không phải một cửa sổ ngữ cảnh đầy.
pub const MAX_LOCATIONS: usize = 100;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub startup: Duration,
    pub request: Duration,
    pub diagnostics: Duration,
    pub max_locations: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            startup: STARTUP_TIMEOUT,
            request: REQUEST_TIMEOUT,
            diagnostics: DIAGNOSTICS_WAIT,
            max_locations: MAX_LOCATIONS,
        }
    }
}

/// Một language server, đúng như người dùng khai nó.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LanguageConfig {
    /// Tên hàng. Cũng là thứ hiện ra trong thông báo lỗi, nên đặt tên người đọc hiểu.
    pub id: String,
    /// Đuôi tệp mà server này nhận. Không có đuôi nào trùng giữa hai hàng — trùng thì hàng
    /// **đầu tiên** thắng, và thứ tự trong bảng là thứ tự người dùng viết ra.
    pub extensions: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// `initializationOptions` gửi kèm lúc bắt tay. Mỗi server hiểu một hình dạng riêng,
    /// nên nó là JSON thô: crate này không có việc gì phải hiểu nội dung của nó.
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Bảng mặc định. Ba hàng, và cả ba đều là những cái tên có thật mà người ta cài sẵn.
pub fn defaults() -> Vec<LanguageConfig> {
    vec![
        LanguageConfig {
            id: "rust".into(),
            extensions: vec!["rs".into()],
            command: "rust-analyzer".into(),
            args: Vec::new(),
            initialization_options: None,
            enabled: true,
        },
        LanguageConfig {
            id: "typescript".into(),
            extensions: vec![
                "ts".into(),
                "tsx".into(),
                "mts".into(),
                "cts".into(),
                "js".into(),
                "jsx".into(),
                "mjs".into(),
                "cjs".into(),
            ],
            command: "typescript-language-server".into(),
            args: vec!["--stdio".into()],
            initialization_options: None,
            enabled: true,
        },
        LanguageConfig {
            id: "python".into(),
            extensions: vec!["py".into(), "pyi".into()],
            command: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            initialization_options: None,
            enabled: true,
        },
    ]
}

/// `languageId` mà spec quy định cho một tệp, tra theo đuôi.
///
/// Tách khỏi [`LanguageConfig`] vì hai thứ này **không** một-một: một
/// `typescript-language-server` phục vụ cả `.ts` lẫn `.js`, nhưng `didOpen` phải khai
/// đúng `"javascript"` cho tệp `.js` — server dựng project suy ra từ chính trường đó.
/// Nhét nó vào hàng cấu hình thì hoặc mỗi đuôi một hàng (hai tiến trình cho một dự án),
/// hoặc một nửa số tệp bị khai sai loại.
pub fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("mts") | Some("cts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js") | Some("mjs") | Some("cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("py") | Some("pyi") => "python",
        Some("go") => "go",
        Some("c") | Some("h") => "c",
        Some("cc") | Some("cpp") | Some("hpp") => "cpp",
        // Không đoán ra thì nói là văn bản thuần. Server sẽ bỏ qua tệp đó, và bỏ qua thì
        // tốt hơn là bị nhận nhầm vào một project mà nó không thuộc về.
        _ => "plaintext",
    }
}
