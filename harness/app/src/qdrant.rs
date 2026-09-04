//! Qdrant sidecar owned by the desktop application.
//!
//! Production never downloads or invokes Docker: the release build embeds the verified
//! executable prepared by `scripts/prepare-sidecars.mjs`. An already-running compatible
//! Qdrant on the configured loopback port is reused and never killed by us.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::Client;
use tokio::sync::Mutex as AsyncMutex;

const READY_ATTEMPTS: usize = 120;
const READY_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct Settings {
    data_dir: PathBuf,
    http_port: u16,
    grpc_port: u16,
    api_key: String,
    executable: Option<PathBuf>,
}

impl Settings {
    fn from_env() -> Self {
        let config = crate::harness::Config::from_env();
        let file = crate::harness::rag_env_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| super::sidecar::parse_env(&text))
            .unwrap_or_default();
        let value = |key: &str, fallback: &str| {
            file.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
                .or_else(|| std::env::var(key).ok())
                .unwrap_or_else(|| fallback.to_owned())
        };
        Self {
            data_dir: config.data_dir.join("qdrant"),
            http_port: value("QDRANT_HTTP_PORT", "6333").parse().unwrap_or(6333),
            grpc_port: value("QDRANT_GRPC_PORT", "6334").parse().unwrap_or(6334),
            api_key: value("QDRANT_API_KEY", ""),
            executable: std::env::var_os("PAI_QDRANT_BIN").map(PathBuf::from),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.http_port)
    }
}

pub(crate) struct ManagedQdrant {
    settings: Settings,
    http: Client,
    start: AsyncMutex<()>,
    child: Mutex<Option<Child>>,
}

impl Default for ManagedQdrant {
    fn default() -> Self {
        Self::new(Settings::from_env())
    }
}

impl ManagedQdrant {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            http: super::sidecar::health_client(),
            start: AsyncMutex::new(()),
            child: Mutex::new(None),
        }
    }

    pub(crate) async fn ensure(&self) -> Result<(), String> {
        if self.ready().await {
            return Ok(());
        }
        let _guard = self.start.lock().await;
        if self.ready().await {
            return Ok(());
        }

        self.reap();
        let executable = match &self.settings.executable {
            Some(path) => super::sidecar::validate(path, "Qdrant")?,
            None => super::sidecar::bundled("qdrant", "Qdrant")?,
        };
        self.spawn(&executable)?;

        for _ in 0..READY_ATTEMPTS {
            if self.ready().await {
                tracing::info!(url = %self.settings.url(), "bundled Qdrant is ready");
                return Ok(());
            }
            if let Some(status) = self.exited()? {
                return Err(format!(
                    "Qdrant vừa khởi động đã dừng ({status}). Xem log tại `{}`",
                    self.log_path().display()
                ));
            }
            tokio::time::sleep(READY_DELAY).await;
        }
        self.stop();
        Err(format!(
            "Qdrant không sẵn sàng tại {} sau {} giây. Xem log tại `{}`",
            self.settings.url(),
            READY_ATTEMPTS as u64 * READY_DELAY.as_millis() as u64 / 1_000,
            self.log_path().display()
        ))
    }

    pub(crate) fn stop(&self) {
        self.reap();
    }

    fn reap(&self) {
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    async fn ready(&self) -> bool {
        let mut request = self.http.get(format!("{}/readyz", self.settings.url()));
        if !self.settings.api_key.is_empty() {
            request = request.header("api-key", &self.settings.api_key);
        }
        request
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }

    fn exited(&self) -> Result<Option<std::process::ExitStatus>, String> {
        let mut child = self.child.lock();
        let Some(child) = child.as_mut() else {
            return Ok(None);
        };
        child
            .try_wait()
            .map_err(|error| format!("không đọc được trạng thái Qdrant: {error}"))
    }

    fn spawn(&self, executable: &Path) -> Result<(), String> {
        let storage = self.settings.data_dir.join("storage");
        let snapshots = self.settings.data_dir.join("snapshots");
        std::fs::create_dir_all(&storage)
            .and_then(|_| std::fs::create_dir_all(&snapshots))
            .map_err(|error| format!("không tạo được kho Qdrant: {error}"))?;
        let (stdout, stderr) = super::sidecar::log_files(&self.log_path(), "Qdrant")?;

        let mut command = Command::new(executable);
        command
            .current_dir(&self.settings.data_dir)
            .env("QDRANT__SERVICE__HOST", "127.0.0.1")
            .env(
                "QDRANT__SERVICE__HTTP_PORT",
                self.settings.http_port.to_string(),
            )
            .env(
                "QDRANT__SERVICE__GRPC_PORT",
                self.settings.grpc_port.to_string(),
            )
            .env("QDRANT__STORAGE__STORAGE_PATH", &storage)
            .env("QDRANT__STORAGE__SNAPSHOTS_PATH", &snapshots)
            .env("QDRANT__TELEMETRY_DISABLED", "true")
            .env("QDRANT__LOG_LEVEL", "INFO")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        if !self.settings.api_key.is_empty() {
            command.env("QDRANT__SERVICE__API_KEY", &self.settings.api_key);
        }
        let child = command
            .spawn()
            .map_err(|error| format!("không chạy được `{}`: {error}", executable.display()))?;
        *self.child.lock() = Some(child);
        Ok(())
    }

    fn log_path(&self) -> PathBuf {
        self.settings.data_dir.join("qdrant.log")
    }
}

impl Drop for ManagedQdrant {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "starts the Qdrant sidecar prepared by scripts/prepare-sidecars.mjs"]
    async fn bundled_qdrant_starts_and_stops() {
        let directory = tempfile::tempdir().expect("temporary Qdrant data");
        let manager = ManagedQdrant::new(Settings {
            data_dir: directory.path().to_owned(),
            http_port: super::super::sidecar::free_port(),
            grpc_port: super::super::sidecar::free_port(),
            api_key: String::new(),
            executable: Some(super::super::sidecar::source_binary("qdrant")),
        });
        manager.ensure().await.expect("bundled Qdrant starts");
        assert!(manager.ready().await);
        manager.stop();
    }
}
