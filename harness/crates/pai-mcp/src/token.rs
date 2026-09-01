//! Chìa khoá của cổng HTTP.
//!
//! Tệp này giữ một bí mật, và bí mật đó **mở được mọi tool trong sổ đăng ký** — `bash`,
//! `write`, `edit`. Ai đọc được nó thì đọc được và sửa được mọi thứ mà ứng dụng chạm tới.
//! Hai hệ quả, và cả hai đều nằm trong mã dưới đây chứ không nằm trong tài liệu:
//!
//! 1. **Quyền `0600`.** Đặt lúc tạo bằng cờ mở tệp, chứ không phải `chmod` sau khi ghi:
//!    giữa hai lời gọi đó có một khoảnh khắc tệp nằm trên đĩa với quyền mặc định.
//! 2. **`data_dir/mcp-token` phải nằm trong danh sách đường dẫn được bảo vệ của
//!    `pai-fs`.** Không có bước đó thì mô hình chỉ cần gọi `read` lên chính cái tệp này là
//!    có đủ thứ để tự gọi lại mọi tool khác, vòng qua mọi lớp canh gác. Xem
//!    [`token_path`].

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Tên tệp trong `data_dir`.
pub const TOKEN_FILE: &str = "mcp-token";

/// Đường dẫn tệp token.
///
/// Hàm này tồn tại để chỗ nối `pai-fs` gọi được nó: `app` dựng danh sách đường dẫn được
/// bảo vệ, và nó phải lấy đường dẫn từ đây thay vì gõ lại chuỗi `"mcp-token"` ở một tệp
/// khác — hai chuỗi giống nhau viết ở hai nơi là hai chuỗi sẽ khác nhau vào một ngày nào
/// đó, và ngày đó cái tệp này thôi được bảo vệ mà không ai thấy.
pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE)
}

/// So sánh hai chuỗi byte trong thời gian không phụ thuộc nội dung.
///
/// `==` của Rust thoát ra ở byte lệch đầu tiên. Với một bí mật, thời gian thoát ra *là*
/// thông tin: kẻ tấn công đo nó và dò ra từng byte một, biến 2^256 khả năng thành 64 lần
/// thử. Vòng lặp dưới đây luôn chạy hết.
///
/// Độ dài thì có rò rỉ, và đó là chấp nhận được: độ dài token là hằng số công khai của
/// chương trình, không phải một phần của bí mật.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

/// Bí mật dùng cho `Authorization: Bearer`.
#[derive(Clone)]
pub struct McpToken {
    value: String,
}

/// Không in ra bí mật, kể cả khi ai đó `dbg!` một struct chứa nó. Một token lọt vào log
/// là một token đã mất.
impl fmt::Debug for McpToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpToken(<đã ẩn>)")
    }
}

impl McpToken {
    /// 256 bit từ CSPRNG, viết ra dạng hex.
    pub fn generate() -> McpToken {
        let bytes: [u8; 32] = rand::random();
        let value = bytes.iter().fold(String::with_capacity(64), |mut out, b| {
            out.push_str(&format!("{b:02x}"));
            out
        });
        McpToken { value }
    }

    /// Dựng từ một giá trị có sẵn. Dành cho bài kiểm chứng và cho cấu hình.
    pub fn from_value(value: impl Into<String>) -> McpToken {
        McpToken {
            value: value.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Token khách trình ra có đúng không. **Luôn** đi qua [`constant_time_eq`].
    pub fn matches(&self, presented: &str) -> bool {
        constant_time_eq(self.value.as_bytes(), presented.as_bytes())
    }

    /// Đọc token cũ, hoặc sinh một cái mới và ghi xuống với quyền `0600`.
    ///
    /// Sinh **một lần** rồi dùng lại: một token đổi sau mỗi lần khởi động buộc mọi client
    /// đã cấu hình phải cấu hình lại, và cách người dùng thoát ra khỏi phiền toái đó
    /// thường là tắt xác thực đi.
    pub fn load_or_create(path: &Path) -> io::Result<McpToken> {
        if let Ok(existing) = fs::read_to_string(path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                harden(path)?;
                return Ok(McpToken::from_value(trimmed));
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let token = McpToken::generate();
        match write_private(path, &token.value) {
            Ok(()) => Ok(token),
            // Một tiến trình khác vừa tạo trước ta. Cái của nó thắng — hai token cùng
            // sống thì một nửa số client sẽ bị từ chối mà không hiểu vì sao.
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let existing = fs::read_to_string(path)?;
                harden(path)?;
                Ok(McpToken::from_value(existing.trim()))
            }
            Err(err) => Err(err),
        }
    }
}

#[cfg(unix)]
fn write_private(path: &Path, value: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    // `create_new` để không đè lên token của một tiến trình khác; `mode` để tệp **sinh ra
    // đã** là 0600, không phải trở thành 0600 một nhịp sau.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(value.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, value: &str) -> io::Result<()> {
    use std::io::Write;

    // Windows không có bit quyền kiểu POSIX; ACL mặc định của thư mục hồ sơ người dùng là
    // thứ duy nhất bảo vệ tệp này. Nói ra ở đây thay vì để im lặng trông như đã xong.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(value.as_bytes())
}

#[cfg(unix)]
fn harden(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        // Siết lại và kêu, chứ không sinh token mới: token cũ có thể đã lộ, nhưng đổi nó
        // ở đây làm mọi client đang chạy đứt kết nối vì một chuyện họ không gây ra. Cái
        // người vận hành cần là một dòng cảnh báo đọc được, để tự quyết định xoá tệp đi.
        tracing::warn!(
            path = %path.display(),
            mode = format!("{:o}", mode & 0o777),
            "tệp token MCP đang mở cho người khác đọc; đã siết về 0600"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn harden(_path: &Path) -> io::Result<()> {
    Ok(())
}
