//! Path <-> `file://` URI, both ways, lossless.
//! Small but the easiest place to be wrong, because a bad URI does not explode: the server
//! returns `null` and the model concludes the symbol does not exist. Percent-encode UTF-8 bytes.

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

/// Characters left alone: RFC 3986's `unreserved` set, plus `/`, which is a separator rather than data.
fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// `/nha/tep ma.rs` -> `file:///nh%C3%A0/t%E1%BB%87p%20m%C3%A3.rs`.
pub fn to_uri(path: &Path) -> Result<String, UriError> {
    let text = path
        .to_str()
        .ok_or_else(|| UriError::NotUtf8(path.to_path_buf()))?;

    let mut uri = String::from("file://");
    // Windows hands us `C:\a\b` with no leading `/`; a URI always has one, so add it here rather than at the call site.
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

/// The reverse; only an empty authority or `localhost` is accepted, since a remote path guessed as local would silently return an unrelated file.
pub fn from_uri(uri: &str) -> Result<PathBuf, UriError> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| UriError::NotFileUri(uri.to_string()))?;

    let encoded = if let Some(tail) = rest.strip_prefix("localhost/") {
        // Drop `localhost` but keep the `/` that opens the path.
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
    // `file:///C:/a` decodes to `/C:/a`; on Windows the leading `/` belongs to the URI, while on Unix `/C:` is a valid directory name.
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
