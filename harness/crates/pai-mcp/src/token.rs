//! The key to the HTTP gate: this secret opens every tool in the registry.
//! So `0600` is set by the open flags, never by a later `chmod`, and `data_dir/mcp-token`
//! must sit in `pai-fs`'s protected paths or `read` hands the model the key.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The filename inside `data_dir`.
pub const TOKEN_FILE: &str = "mcp-token";

/// The token file's path; exported so the protected-paths list takes it from here instead of retyping the name.
pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE)
}

/// Compare bytes in content-independent time; `==` returns at the first mismatch, which leaks the secret byte by byte.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

/// The secret used for `Authorization: Bearer`.
#[derive(Clone)]
pub struct McpToken {
    value: String,
}

/// Never print the secret, even through a `dbg!` of some enclosing struct: a logged token is a lost token.
impl fmt::Debug for McpToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpToken(<đã ẩn>)")
    }
}

impl McpToken {
    /// 256 bits from the CSPRNG, written as hex.
    pub fn generate() -> McpToken {
        let bytes: [u8; 32] = rand::random();
        let value = bytes.iter().fold(String::with_capacity(64), |mut out, b| {
            out.push_str(&format!("{b:02x}"));
            out
        });
        McpToken { value }
    }

    /// Build from an existing value; for tests and for configuration.
    pub fn from_value(value: impl Into<String>) -> McpToken {
        McpToken {
            value: value.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Whether the presented token matches; always through [`constant_time_eq`].
    pub fn matches(&self, presented: &str) -> bool {
        constant_time_eq(self.value.as_bytes(), presented.as_bytes())
    }

    /// Read the existing token, or generate one and write it `0600`; rotating on every start makes users disable auth.
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
            // Another process created it first and wins; two live tokens would reject half the clients.
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

    // `create_new` avoids clobbering another process's token; `mode` makes the file 0600 from birth.
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

    // Windows has no POSIX mode bits; the profile directory's default ACL is all that protects this file.
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
        // Tighten and warn rather than rotate: rotating here disconnects every running client over something they did not do.
        tracing::warn!(
            path = %path.display(),
            mode = format!("{:o}", mode & 0o777),
            "the MCP token file was world-readable; tightened to 0600"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn harden(_path: &Path) -> io::Result<()> {
    Ok(())
}
