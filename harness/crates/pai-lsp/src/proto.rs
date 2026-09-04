//! LSP's JSON-RPC framing: `Content-Length`, a blank line, then the body.
//! Headers are read leniently and bodies strictly, because header variation is an
//! implementation difference; the length is capped, since it comes from another process.

use std::io;

use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};

/// Cap for one message; a long generic hover can reach hundreds of KB, and anything past 32 MB is garbage rather than a big answer.
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

/// `Ok(None)` means the pipe ended - the server closed stdout, so it has exited.
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
