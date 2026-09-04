//! SurrealDB sidecar bundled with production installers.

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
    port: u16,
    executable: Option<PathBuf>,
}

impl Settings {
    fn from_env() -> Self {
        let config = crate::harness::Config::from_env();
        let file = crate::harness::rag_env_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| super::sidecar::parse_env(&text))
            .unwrap_or_default();
        let port = file
            .iter()
            .find(|(name, _)| name == "SURREAL_HTTP_PORT")
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var("SURREAL_HTTP_PORT").ok())
            .and_then(|value| value.parse().ok())
            .unwrap_or(8000);
        Self {
            data_dir: config.data_dir.join("surrealdb"),
            port,
            executable: std::env::var_os("PAI_SURREAL_BIN").map(PathBuf::from),
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub(crate) struct ManagedSurrealDb {
    settings: Settings,
    http: Client,
    start: AsyncMutex<()>,
    child: Mutex<Option<Child>>,
}

impl Default for ManagedSurrealDb {
    fn default() -> Self {
        Self::new(Settings::from_env())
    }
}

impl ManagedSurrealDb {
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
            Some(path) => super::sidecar::validate(path, "SurrealDB")?,
            None => super::sidecar::bundled("surreal", "SurrealDB")?,
        };
        self.spawn(&executable)?;

        for _ in 0..READY_ATTEMPTS {
            if self.ready().await {
                tracing::info!(url = %self.settings.url(), "bundled SurrealDB is ready");
                return Ok(());
            }
            if let Some(status) = self.exited()? {
                return Err(format!(
                    "SurrealDB vừa khởi động đã dừng ({status}). Xem log tại `{}`",
                    self.log_path().display()
                ));
            }
            tokio::time::sleep(READY_DELAY).await;
        }
        self.stop();
        Err(format!(
            "SurrealDB không sẵn sàng tại {}. Xem log tại `{}`",
            self.settings.url(),
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
        self.http
            .get(format!("{}/ready", self.settings.url()))
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
            .map_err(|error| format!("không đọc được trạng thái SurrealDB: {error}"))
    }

    fn spawn(&self, executable: &Path) -> Result<(), String> {
        std::fs::create_dir_all(&self.settings.data_dir)
            .map_err(|error| format!("không tạo được kho SurrealDB: {error}"))?;
        let (stdout, stderr) = super::sidecar::log_files(&self.log_path(), "SurrealDB")?;
        let child = Command::new(executable)
            .current_dir(&self.settings.data_dir)
            .args([
                "start",
                "--no-banner",
                "--log",
                "info",
                "--bind",
                &format!("127.0.0.1:{}", self.settings.port),
                "--unauthenticated",
                "surrealkv:storage",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| format!("không chạy được `{}`: {error}", executable.display()))?;
        *self.child.lock() = Some(child);
        Ok(())
    }

    fn log_path(&self) -> PathBuf {
        self.settings.data_dir.join("surrealdb.log")
    }
}

impl Drop for ManagedSurrealDb {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "starts the SurrealDB sidecar prepared by scripts/prepare-sidecars.mjs"]
    async fn bundled_surrealdb_starts_and_stops() {
        let directory = tempfile::tempdir().expect("temporary SurrealDB data");
        let manager = ManagedSurrealDb::new(Settings {
            data_dir: directory.path().to_owned(),
            port: super::super::sidecar::free_port(),
            executable: Some(super::super::sidecar::source_binary("surreal")),
        });
        manager.ensure().await.expect("bundled SurrealDB starts");
        assert!(manager.ready().await);
        manager.stop();
    }
}
