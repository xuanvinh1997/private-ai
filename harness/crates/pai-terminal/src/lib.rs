//! Terminal bền: một seam, sáu tool, một phiên sống lâu hơn lượt sinh ra nó.
//!
//! # Vì sao đây không phải là `bash` có thêm một cờ
//!
//! `bash` chạy một lệnh rồi trả kết quả. Tiến trình chết, và cùng với nó chết mọi thứ nó
//! biết: thư mục hiện tại, biến môi trường vừa `export`, hàm vừa định nghĩa, cái REPL vừa
//! mở, cái máy chủ vừa dựng. Lần gọi sau bắt đầu lại từ đầu.
//!
//! Terminal **giữ một phiên sống**. `cd src` ở lần gọi này còn tác dụng ở lần gọi sau.
//! `python3` mở ra rồi vẫn ở đó, còn nhớ biến của lần trước. `npm run dev` vẫn chạy và vẫn
//! in ra, và đọc nó không cần phải đoán xem nó đã chết chưa.
//!
//! Hai thứ đó là **hai loại việc**, không phải hai mức tiện lợi của một loại việc. Một
//! lệnh không trạng thái chạy trong một phiên bền thì phải tự dọn sau mình, còn một việc
//! có trạng thái chạy qua `bash` thì đơn giản là không làm được. Nên chúng là hai bộ tool
//! với hai vòng đời khác nhau, chứ không phải một bộ với một cờ `persistent` — một cờ như
//! thế đặt lên vai người gọi câu hỏi "lần này tôi có cần trạng thái không", hỏi lại ở mọi
//! lần gọi, và trả lời sai thì không có gì báo.
//!
//! # Vì sao PTY thật chứ không phải ống dẫn
//!
//! Rất nhiều công cụ **đổi hành vi** khi thấy đầu ra không phải terminal. `ls` bỏ màu và
//! đổi sang một cột. `git log` không gọi pager. `cargo`, `npm`, `pip` tắt thanh tiến
//! trình. `python` không vào chế độ tương tác. Một tiện ích hỏi "y/n" có thể bỏ luôn câu
//! hỏi và chọn mặc định. Không có cái nào trong số đó là lỗi: chương trình đang làm đúng
//! điều nó được dạy làm khi không có ai ngồi xem.
//!
//! Hệ quả là một agent nhìn qua ống dẫn thấy **một thế giới khác** với thế giới người dùng
//! thấy. Khi hai bên dán output cho nhau — người dùng dán một lỗi có màu và có thanh tiến
//! trình, agent dán lại một khối chữ phẳng — họ đang nói về hai thứ, và cả hai đều không
//! biết. Cách sửa không phải là dạy agent phiên dịch, mà là cho nó đúng cái thiết bị mà
//! người dùng có: một PTY thật, `isatty` trả về đúng, `SIGWINCH` tới nơi, `TERM` có nghĩa.
//!
//! # Ba bất biến, và cả ba đều đã có chỗ để làm sai
//!
//! **Phiên thuộc về ai tạo ra nó.** Mỗi phiên ghi lại [`Owner`] — phạm vi của `pai-core`
//! — lúc mở, và mọi lời gọi sau đó phải trình đúng chủ. Một agent con hỏi id phiên của
//! agent khác nhận đúng câu trả lời như khi hỏi một id không tồn tại. Xem [`provider`].
//!
//! **Phiên chết cùng plugin.** Một shell sống lâu hơn thứ sinh ra nó là một shell không ai
//! còn nhớ để dọn, và nó vẫn giữ cổng, giữ khoá tệp, ghi tiếp vào thư mục làm việc. Gỡ
//! plugin đóng sạch, kể cả tiến trình cháu — cùng bài học và cùng cách làm như `pai-shell`.
//!
//! **Bộ đệm có trần.** Một máy chủ phát triển in ra hàng giờ. Giữ hết là để cho nó ăn dần
//! bộ nhớ cho tới lúc ứng dụng chết; cắt bỏ trong im lặng là để mô hình kết luận trên một
//! bản ghi thiếu mà không biết là thiếu. Nên bộ đệm giữ phần **mới nhất** và **nói ra** đã
//! bỏ bao nhiêu dòng. Xem [`buffer`].

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
