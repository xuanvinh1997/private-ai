//! Shared, deliberately small sidecar plumbing.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .unwrap_or(value);
            (key.trim().to_owned(), value.to_owned())
        })
        .collect()
}

pub(crate) fn health_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("sidecar health client uses valid fixed timeouts")
}

pub(crate) fn validate(path: &Path, label: &str) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path.to_owned())
    } else {
        Err(format!(
            "không thấy binary {label} tại `{}`",
            path.display()
        ))
    }
}

/// Tauri strips the target suffix and installs sidecars beside the application executable. The source-tree
/// candidate keeps `cargo test` and direct development runs useful after the preparation script has run.
pub(crate) fn bundled(name: &str, label: &str) -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("không xác định được executable ứng dụng: {error}"))?;
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let installed = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{name}{suffix}"));
    if installed.is_file() {
        return Ok(installed);
    }
    let source = source_binary(name);
    validate(&source, label).map_err(|_| {
        format!(
            "bản cài thiếu binary {label}. Dựng lại bằng `node scripts/package.mjs`; đã tìm `{}` và `{}`",
            installed.display(),
            source.display()
        )
    })
}

pub(crate) fn source_binary(name: &str) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "binaries/{name}-{}{suffix}",
        env!("TAURI_ENV_TARGET_TRIPLE")
    ))
}

pub(crate) fn log_files(path: &Path, label: &str) -> Result<(File, File), String> {
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("không mở được log {label}: {error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("không mở được stderr {label}: {error}"))?;
    Ok((stdout, stderr))
}

#[cfg(test)]
pub(crate) fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("bind probe port")
        .local_addr()
        .expect("probe address")
        .port()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_parser_matches_compose_basics() {
        assert_eq!(
            parse_env("# x\nQDRANT_HTTP_PORT=7444\nQDRANT_API_KEY=\"secret\"\n"),
            vec![
                ("QDRANT_HTTP_PORT".into(), "7444".into()),
                ("QDRANT_API_KEY".into(), "secret".into()),
            ]
        );
    }

    #[test]
    fn source_names_follow_tauri_external_binary_contract() {
        let path = source_binary("qdrant");
        let file = path.file_name().unwrap().to_string_lossy();
        assert!(file.starts_with("qdrant-"));
        assert!(file.contains(env!("TAURI_ENV_TARGET_TRIPLE")));
        assert_eq!(file.ends_with(".exe"), cfg!(windows));
    }
}
