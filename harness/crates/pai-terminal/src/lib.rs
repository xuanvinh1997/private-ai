//! Terminal bền: một seam, sáu tool, một phiên sống lâu hơn lượt sinh ra nó.
//!
//! # Vì sao đây không phải `bash` có thêm một cờ
//!
//! `bash` chạy một lệnh rồi trả kết quả; tiến trình chết cùng mọi thứ nó biết — thư mục
//! hiện tại, biến vừa `export`, hàm vừa định nghĩa, REPL vừa mở, máy chủ vừa dựng.
//! Terminal **giữ một phiên sống**: `cd src` còn tác dụng ở lần gọi sau, `python3` vẫn nhớ
//! biến của lần trước, `npm run dev` vẫn chạy và vẫn in ra.
//!
//! Đó là **hai loại việc**, không phải hai mức tiện lợi của một loại. Một lệnh không trạng
//! thái chạy trong phiên bền thì phải tự dọn sau mình; một việc có trạng thái chạy qua
//! `bash` thì đơn giản là không làm được. Nên hai bộ tool với hai vòng đời, chứ không phải
//! một cờ `persistent` — cờ ấy đặt lên vai người gọi câu hỏi "lần này tôi có cần trạng thái
//! không", hỏi lại ở mọi lần gọi, và trả lời sai thì không có gì báo.
//!
//! # Vì sao PTY thật chứ không phải ống dẫn
//!
//! Rất nhiều công cụ **đổi hành vi** khi đầu ra không phải terminal: `ls` bỏ màu và đổi một
//! cột, `git log` không gọi pager, `cargo`/`npm`/`pip` tắt thanh tiến trình, `python` không
//! vào chế độ tương tác, một tiện ích hỏi "y/n" có thể bỏ luôn câu hỏi. Không cái nào là
//! lỗi: chúng làm đúng điều được dạy làm khi không có ai ngồi xem.
//!
//! Hệ quả là agent nhìn qua ống dẫn thấy **một thế giới khác** với thế giới người dùng
//! thấy, và khi hai bên dán output cho nhau thì cả hai đều không biết. Cách sửa không phải
//! dạy agent phiên dịch, mà là cho nó đúng thiết bị người dùng có: PTY thật, `isatty` trả
//! về đúng, `SIGWINCH` tới nơi, `TERM` có nghĩa.
//!
//! # Ba bất biến
//!
//! **Phiên thuộc về ai tạo ra nó.** Mỗi phiên ghi [`Owner`] lúc mở, và mọi lời gọi sau phải
//! trình đúng chủ; hỏi id phiên của agent khác nhận đúng câu trả lời như hỏi id không tồn
//! tại. Xem [`provider`].
//!
//! **Phiên chết cùng plugin.** Một shell sống lâu hơn thứ sinh ra nó vẫn giữ cổng, giữ khoá
//! tệp, ghi tiếp vào thư mục làm việc. Gỡ plugin đóng sạch, kể cả tiến trình cháu — cùng
//! cách làm như `pai-shell`.
//!
//! **Bộ đệm có trần.** Giữ hết đầu ra của một máy chủ phát triển là ăn dần bộ nhớ tới lúc
//! ứng dụng chết; cắt trong im lặng là để mô hình kết luận trên bản ghi thiếu mà không biết.
//! Nên bộ đệm giữ phần **mới nhất** và **nói ra** đã bỏ bao nhiêu dòng. Xem [`buffer`].

pub mod buffer;
pub mod plugin;
pub mod provider;
pub mod seam;
pub mod session;
pub mod tools;

pub use buffer::{Page, Ring};
pub use plugin::{TerminalPlugin, register_tools};
pub use provider::{LocalTerminals, SHELL_BACKEND};
pub use seam::{
    DEFAULT_COLS, DEFAULT_MAX_LINES, DEFAULT_ROWS, OpenRequest, Owner, Sent, SessionInfo, Signal,
    Stop, TerminalError, TerminalHost, Terminals, Wait,
};
pub use tools::{
    TerminalClose, TerminalList, TerminalOpen, TerminalRead, TerminalSend, TerminalSignal,
};
