//! Đường dẫn ↔ `file://` URI, hai chiều, không mất mát.
//!
//! Tệp này nhỏ nhưng nó là chỗ dễ sai nhất trong crate, vì sai ở đây **không nổ**. Một
//! đường dẫn có khoảng trắng hay dấu tiếng Việt đi qua một phép nối chuỗi ngây thơ sẽ ra
//! một URI mà language server không nhận ra; server trả về `null`, tool nói "không có
//! định nghĩa nào", và mô hình tin rằng hàm đó không tồn tại. Một câu trả lời sai trông y
//! hệt một câu trả lời đúng — nên phép chuyển này được viết ra thành hàm riêng và được
//! khoá bằng bài kiểm chứng khứ hồi, thay vì rải `format!("file://{}")` khắp nơi.
//!
//! Hai quyết định:
//!
//! - **Mã hoá theo byte của UTF-8, không theo ký tự.** RFC 3986 định nghĩa phần trăm-mã
//!   hoá trên octet; `%C6%B0` là "ư", không phải hai ký tự.
//! - **Đường dẫn không phải UTF-8 là lỗi, không phải mất mát.** `to_string_lossy` sẽ đổi
//!   byte lạ thành `U+FFFD`, và cái URI ra lò trỏ vào một tệp *khác* — hoặc không tệp
//!   nào. Nói thẳng là không chuyển được thì tool trả về một câu người đọc hiểu.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum UriError {
    #[error("đường dẫn {0} không phải UTF-8 nên không chuyển sang URI được")]
    NotUtf8(PathBuf),
    #[error("`{0}` không phải một URI `file://`")]
    NotFileUri(String),
    #[error("`{0}` trỏ tới máy khác; harness chỉ đọc tệp trên máy này")]
    RemoteHost(String),
    #[error("`{0}` có một dãy phần trăm-mã hoá hỏng")]
    BadEscape(String),
}

/// Ký tự được để nguyên: đúng tập `unreserved` của RFC 3986, cộng `/` vì nó là dấu ngăn
/// đoạn chứ không phải dữ liệu.
fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// `/nhà/tệp mã.rs` → `file:///nh%C3%A0/t%E1%BB%87p%20m%C3%A3.rs`.
pub fn to_uri(path: &Path) -> Result<String, UriError> {
    let text = path
        .to_str()
        .ok_or_else(|| UriError::NotUtf8(path.to_path_buf()))?;

    let mut uri = String::from("file://");
    // Windows đưa vào `C:\a\b`, không có `/` đứng đầu; URI thì luôn có. Thêm nó ở đây
    // chứ không ở chỗ gọi, để chỗ gọi không phải biết mình đang chạy trên hệ nào.
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'/' => uri.push('/'),
            b'\\' if cfg!(windows) => uri.push('/'),
            b if unreserved(b) => uri.push(char::from(b)),
            b => uri.push_str(&format!("%{b:02X}")),
        }
    }
    Ok(uri)
}

/// Chiều ngược lại. Chỉ nhận authority rỗng hoặc `localhost`.
///
/// `file://may-khac/duong/dan` là một tệp trên máy khác. Ta không đọc được nó, và đoán
/// rằng nó nằm ở `/duong/dan` trên máy này là cách trả về nội dung của một tệp không liên
/// quan mà không ai biết.
pub fn from_uri(uri: &str) -> Result<PathBuf, UriError> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| UriError::NotFileUri(uri.to_string()))?;

    let encoded = if let Some(tail) = rest.strip_prefix("localhost/") {
        // Bỏ `localhost` nhưng giữ lại dấu `/` mở đầu đường dẫn.
        &rest[rest.len() - tail.len() - 1..]
    } else if rest.starts_with('/') {
        rest
    } else if rest.is_empty() {
        return Err(UriError::NotFileUri(uri.to_string()));
    } else {
        return Err(UriError::RemoteHost(uri.to_string()));
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(encoded.len());
    let mut chars = encoded.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let high = chars.next().and_then(|c| c.to_digit(16));
        let low = chars.next().and_then(|c| c.to_digit(16));
        match (high, low) {
            (Some(high), Some(low)) => bytes.push((high * 16 + low) as u8),
            _ => return Err(UriError::BadEscape(uri.to_string())),
        }
    }

    let text = String::from_utf8(bytes).map_err(|_| UriError::BadEscape(uri.to_string()))?;
    // `file:///C:/a` giải ra `/C:/a`; trên Windows dấu `/` đầu là của URI, không của
    // đường dẫn. Trên Unix `/C:` là một tên thư mục hợp lệ nên không được đụng vào.
    #[cfg(windows)]
    let text = {
        let bytes = text.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            text[1..].to_string()
        } else {
            text
        }
    };
    Ok(PathBuf::from(text))
}
