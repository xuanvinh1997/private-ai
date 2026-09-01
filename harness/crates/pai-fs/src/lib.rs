//! Hệ tệp: một seam, một chính sách, năm tool.
//!
//! Ba ý đáng nhớ:
//!
//! **Chuẩn hoá trước, kiểm tra sau.** Kiểm trước khi chuẩn hoá nghĩa là
//! `gốc/../../etc/passwd` đi lọt, vì lúc so khớp nó vẫn còn bắt đầu bằng `gốc/`. Xem
//! [`path`].
//!
//! **Chính sách không nằm trong tool.** Luật đọc-trước-khi-sửa là một middleware trên
//! đường ống của `pai-tools`, nên `edit` không biết luật đó tồn tại, tắt luật là gỡ một
//! plugin, và một tool ghi tệp viết sau này tự động chịu luật. Xem [`observed`].
//!
//! **Tool không gọi `std::fs`.** Chúng gọi qua [`provider::Fs`], nên trỏ provider vào một
//! sandbox là cả năm tool đi theo mà không tool nào phải sửa.

pub mod observed;
pub mod path;
pub mod plugin;
pub mod provider;
pub mod tools;

pub use observed::{ReadBeforeEdit, ReadLedger};
pub use path::{FileRoots, PathError, looks_binary};
pub use plugin::FsPlugin;
pub use provider::{Fs, FsError, FsProvider, LocalFs};
