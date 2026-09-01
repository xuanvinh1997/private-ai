//! Khung tin JSON-RPC của LSP: `Content-Length`, dòng trống, rồi thân tin.
//!
//! Đây là phần duy nhất của giao thức mà mọi language server đều giống nhau tuyệt đối, và
//! nó gọn tới mức viết ra rẻ hơn nuôi một dependency. Hai điều đáng nói:
//!
//! **Header đọc khoan dung, thân tin đọc chặt.** Một server gửi thêm `Content-Type` hay
//! viết hoa khác đi vẫn phải chạy được; một thân tin không phải JSON thì không. Chỗ đầu
//! là khác biệt giữa các bản cài đặt, chỗ sau là hỏng thật.
//!
//! **Có trần độ dài.** `Content-Length` đến từ một tiến trình bên ngoài, và cấp phát theo
//! một con số do bên ngoài đọc là cách một server hỏng làm ứng dụng hết bộ nhớ.

use std::io;

use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};

/// Trần cho một tin. Hover của `rust-analyzer` trên một kiểu generic dài có thể tới vài
/// trăm KB; ba mươi hai MB thì không có gì thật chạm tới, và một con số ngoài khoảng đó
/// là dấu hiệu của rác chứ không phải của một câu trả lời lớn.
pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

pub async fn write_message<W>(sink: &mut W, message: &Value) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(message)?;
    sink.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    sink.write_all(&body).await?;
    sink.flush().await
}

/// `Ok(None)` là hết ống — server đã đóng stdout, tức là nó đã thoát.
pub async fn read_message<R>(source: &mut R) -> io::Result<Option<Value>>
where
    R: AsyncBufRead + AsyncRead + Unpin,
{
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if source.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }

    let length = length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "tin không có `Content-Length`")
    })?;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("tin dài {length} byte, quá trần {MAX_MESSAGE_BYTES}"),
        ));
    }

    let mut body = vec![0u8; length];
    source.read_exact(&mut body).await?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}
