//! The `pai-rag-service` child process and the MCP connection to it.
//! Connects lazily on the first call and reconnects exactly once on a closed pipe.
//! Child stderr is inherited so a failing service can say why; stdout is the JSON-RPC line.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use serde_json::{Map, Value};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::RagError;

/// How to start the service.
#[derive(Clone, Debug)]
pub struct SidecarConfig {
    /// Command that runs the service. Usually `uv`; a packaged build will point at an executable.
    pub command: String,
    pub args: Vec<String>,
    /// Working directory of the child. With `uv` this is where `pyproject.toml` lives.
    pub cwd: Option<PathBuf>,
    /// Added to the child's environment rather than replacing it.
    pub env: BTreeMap<String, String>,
    /// Project id, sent with every call; passing it explicitly avoids two places remembering which project is open.
    pub project: String,
}

impl SidecarConfig {
    /// Run the service with `uv` from the repo source - the development path; `uv` fetches Python 3.12 itself.
    pub fn uv(service_dir: impl Into<PathBuf>, project: impl Into<String>) -> SidecarConfig {
        SidecarConfig {
            command: "uv".to_string(),
            args: ["run", "--python", "3.12", "pai-rag", "serve"]
                .iter()
                .map(|part| part.to_string())
                .collect(),
            cwd: Some(service_dir.into()),
            env: BTreeMap::new(),
            project: project.into(),
        }
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> SidecarConfig {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// Connection to the service: opened lazily, reconnectable.
pub struct Sidecar {
    config: SidecarConfig,
    /// `AsyncMutex`, not `parking_lot`: the connect step is `async` and holding a sync lock across `.await` stalls the runtime.
    running: AsyncMutex<Option<Arc<RunningService<RoleClient, ()>>>>,
    /// Last failure reason, so `stats()` can say why the library is silent instead of just reporting zero.
    last_error: Mutex<Option<String>>,
}

impl Sidecar {
    pub fn new(config: SidecarConfig) -> Sidecar {
        Sidecar {
            config,
            running: AsyncMutex::new(None),
            last_error: Mutex::new(None),
        }
    }

    pub fn project(&self) -> &str {
        &self.config.project
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().clone()
    }

    /// Call a tool, reconnecting once if the pipe is closed.
    pub async fn call(&self, tool: &str, mut args: Map<String, Value>) -> Result<Value, RagError> {
        // Every service tool takes `project`; fill it here rather than at seven call sites.
        args.insert("project".to_string(), Value::String(self.config.project.clone()));

        match self.call_once(tool, args.clone()).await {
            Ok(value) => {
                *self.last_error.lock() = None;
                Ok(value)
            }
            Err(first) => {
                tracing::debug!(%first, tool, "service call failed, reconnecting");
                self.reset().await;
                match self.call_once(tool, args).await {
                    Ok(value) => {
                        *self.last_error.lock() = None;
                        Ok(value)
                    }
                    Err(err) => {
                        *self.last_error.lock() = Some(err.to_string());
                        Err(err)
                    }
                }
            }
        }
    }

    async fn call_once(&self, tool: &str, args: Map<String, Value>) -> Result<Value, RagError> {
        let service = self.connect().await?;
        let mut params = CallToolRequestParams::new(tool.to_string());
        if !args.is_empty() {
            params = params.with_arguments(args);
        }
        match service
            .call_tool_once(params)
            .await
            .map_err(|err| RagError::Service(format!("gọi `{tool}` hỏng: {err}")))?
        {
            CallToolResponse::Complete(result) => read_result(tool, result),
            // The server asked the user something mid-call. Ours never does, so the protocols have drifted apart.
            other => Err(RagError::Service(format!(
                "`{tool}` trả về phản hồi không mong đợi: {other:?}"
            ))),
        }
    }

    /// The current connection, opened if there is none.
    async fn connect(&self) -> Result<Arc<RunningService<RoleClient, ()>>, RagError> {
        let mut slot = self.running.lock().await;
        if let Some(found) = slot.as_ref() {
            return Ok(found.clone());
        }

        let mut command = tokio::process::Command::new(&self.config.command);
        command.args(&self.config.args);
        // Force UTF-8 on the child *before* config, so Windows cp1252 stdio cannot truncate messages.
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("PYTHONUTF8", "1");
        for (key, value) in &self.config.env {
            command.env(key, value);
        }
        if let Some(dir) = &self.config.cwd {
            command.current_dir(dir);
        }

        // `None` for stderr = inherit the parent's stderr. See the module docstring.
        let (transport, _stderr) = TokioChildProcess::builder(command)
            .spawn()
            .map_err(|err| {
                RagError::Service(format!(
                    "không chạy được `{}`: {err}. Cài `uv` (https://docs.astral.sh/uv) \
                     hoặc kiểm tra đường dẫn service trong cài đặt.",
                    self.config.command
                ))
            })?;

        // Bounded, because the first `uv run` builds the environment and downloads Python - slow, but it must not hang forever.
        let serving = tokio::time::timeout(CONNECT_TIMEOUT, ().serve(transport))
            .await
            .map_err(|_| {
                RagError::Service(format!(
                    "service không trả lời trong {} giây. Lần chạy đầu `uv` phải tải \
                     Python và dựng môi trường — thử chạy `uv sync` trong services/rag \
                     một lần trước.",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|err| RagError::Service(format!("bắt tay MCP hỏng: {err}")))?;

        let shared = Arc::new(serving);
        *slot = Some(shared.clone());
        tracing::info!(project = %self.config.project, "connected to pai-rag-service");
        Ok(shared)
    }

    /// Drop the current connection. The next call reopens it.
    async fn reset(&self) {
        let mut slot = self.running.lock().await;
        *slot = None;
    }

    /// Close the connection and the child process. Called when the plugin is unplugged.
    pub async fn shutdown(&self) {
        let taken = { self.running.lock().await.take() };
        let Some(service) = taken else { return };
        // Only stop when we are the last owner; an in-flight call still holds an `Arc`.
        match Arc::try_unwrap(service) {
            Ok(owned) => {
                if let Err(err) = owned.cancel().await {
                    tracing::debug!(%err, "error while shutting down pai-rag-service");
                }
            }
            Err(_) => tracing::debug!("call still in flight, letting the process close itself"),
        }
    }
}

/// The first `uv` run has to download Python and build a virtualenv.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);

/// Pull the structured payload out of a `CallToolResult`; read `structured_content` rather than parsing the human-readable text.
fn read_result(tool: &str, result: CallToolResult) -> Result<Value, RagError> {
    if result.is_error.unwrap_or(false) {
        let detail = result
            .content
            .iter()
            .filter_map(|block| block.as_text().map(|text| text.text.clone()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(RagError::Service(format!("`{tool}`: {detail}")));
    }
    result.structured_content.ok_or_else(|| {
        RagError::Service(format!("`{tool}` không trả về dữ liệu có cấu trúc"))
    })
}
