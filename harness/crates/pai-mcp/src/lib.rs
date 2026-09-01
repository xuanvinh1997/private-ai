//! MCP: hai chiều của cùng một ranh giới, và ranh giới đó là ranh giới tin cậy.
//!
//! **Ra ngoài** ([`hub`]) — ta là client của server bên thứ ba. Cái đi vào là tool do
//! người khác viết, chạy trên máy người dùng, trả về văn bản do người khác soạn.
//!
//! **Vào trong** ([`expose`]) — ta là server, phơi [`pai_tools::ToolRegistry`] ra cho một
//! client khác. Cái đi ra là toàn bộ khả năng của ứng dụng.
//!
//! Ba bất biến, và cả ba đều là lý do chứ không phải hệ quả:
//!
//! **1. Tiền tố `ext.<server>.` đặt vào trước khi ai nhìn thấy.** Không phải lúc hiển
//! thị, không phải lúc ghi log — mà ngay tại chỗ danh sách tool từ xa được đọc về, trước
//! khi nó chạm vào sổ đăng ký. Một tool bên thứ ba vì thế không trùng tên được với tool
//! nội bộ, và cũng không giả dạng được một cái tên mà chính sách sẽ đối xử tử tế hơn.
//! Chiều ngược lại — cắt tiền tố — xảy ra ở đúng một chỗ, ngay trước khi chuyển tiếp.
//! Xem [`naming`].
//!
//! **2. Giả định xấu nhất về tool bên ngoài.** `mutating` và `returns_untrusted_content`
//! đều bật, bất kể server tự khai gì. Xem [`remote::RemoteTool::meta`].
//!
//! **3. Best-effort.** Một server bên thứ ba chết, treo, hay trả về rác không được làm
//! người dùng mất tool của chính họ. Mọi lỗi ở phía ngoài dừng lại trong task giám sát của
//! đúng server đó.
//!
//! ## Vòng đời một kết nối — và chỗ bản Rust không cần chép bản Python
//!
//! Bản Python (`mcp/client.py:110-116`) phải nhốt cả kết nối vào **một task duy nhất**:
//! context manager của transport bị ràng buộc theo task của anyio, nên mở ở task này rồi
//! đóng từ task khác là tháo một cancel scope ở sai chỗ. Cấu trúc "task mở, báo sẵn sàng,
//! rồi ngồi chờ tín hiệu dừng" ở đó là một cách **né** một hạn chế của runtime.
//!
//! Ở đây không có hạn chế đó, nên không chép cách né. Quyền sở hữu được nói thẳng bằng hệ
//! kiểu: [`rmcp::service::RunningService`] có đúng một chủ — task giám sát —, còn phần
//! mọi nơi khác cầm là [`rmcp::service::Peer`], một handle `Clone + Send` gửi được từ bất
//! kỳ task nào. Dừng một kết nối là `CancellationToken::cancel()` từ bất cứ đâu; dọn dẹp
//! vẫn chạy trong task sở hữu nó. Không có tín hiệu bằng tay, không có `Event` để quên
//! set, và không có đường nào đóng nhầm chỗ.

pub mod catalog;
pub mod config;
pub mod dial;
pub mod expose;
pub mod hub;
pub mod naming;
pub mod plugin;
pub mod remote;
pub mod seam;
pub mod serve;
pub mod store;
pub mod token;

pub use catalog::{CATALOG, CatalogEntry, EnvVar, instantiate};
pub use config::{ConfigError, McpTransport, ServerConfig};
pub use dial::{ConfigDialers, Dialer, DialerFactory, Reach};
pub use expose::RegistryServer;
pub use hub::{McpHub, Mount, RetryPolicy, ServerState, ServerStatus};
pub use naming::{EXTERNAL_PREFIX, is_external, namespace, qualify, remote_of};
pub use plugin::{ExposeOptions, McpPlugin};
pub use remote::{Link, RemoteTool};
pub use seam::{Mcp, McpConfig};
pub use serve::{Denied, HttpEndpoint, HttpGuard, serve_http, serve_stdio};
pub use store::{McpStore, StoreError, apply, merge};
pub use token::{McpToken, TOKEN_FILE, constant_time_eq, token_path};
