//! Tiến trình `pai-rag-service` và kết nối MCP tới nó.
//!
//! # Nối lười, và nối lại
//!
//! Kết nối được mở ở **lần gọi đầu tiên**, không phải lúc cắm plugin. Lý do là thời điểm:
//! plugin được cắm ngay khi mở dự án, còn tiến trình Python mất một hai giây để khởi
//! động và có thể chưa được cài. Mở sớm nghĩa là mọi lần mở dự án đều trả giá đó, kể cả
//! khi người dùng chỉ định trò chuyện chứ không đụng tới thư viện.
//!
//! Một lời gọi hỏng vì ống đã đóng thì [`Sidecar::call`] **nối lại đúng một lần** rồi thử
//! lại. Một lần, không phải một vòng lặp: nếu lần nối lại cũng hỏng thì vấn đề không phải
//! ở kết nối, và thử tiếp chỉ kéo dài thời gian người dùng ngồi nhìn một ô đang quay.
//!
//! # stderr đi thẳng ra stderr của ta
//!
//! Một service hỏng gần như luôn nói lý do ở đó — thiếu Python, thiếu gói, cấu hình sai
//! đường dẫn. Nuốt nó đi là biến "không nối được" thành một bí ẩn. stdout thì **không**
//! được đụng vào: đó là đường JSON-RPC.

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

/// Cách khởi động service.
#[derive(Clone, Debug)]
pub struct SidecarConfig {
    /// Lệnh chạy service. Thường là `uv`; một bản đóng gói sau này sẽ là đường dẫn tới
    /// một tệp thi hành.
    pub command: String,
    pub args: Vec<String>,
    /// Thư mục làm việc của tiến trình con. Với `uv` thì đây là chỗ có `pyproject.toml`.
    pub cwd: Option<PathBuf>,
    /// Thêm vào môi trường của tiến trình con, không thay thế nó.
    pub env: BTreeMap<String, String>,
    /// Mã dự án, gửi kèm **mọi** lời gọi.
    ///
    /// Gửi tường minh chứ không dựa vào "dự án đang mở" trong tệp cấu hình: hai chỗ cùng
    /// nhớ một trạng thái thì có lúc chúng lệch nhau, và lúc ấy người dùng nhận về đoạn
    /// văn của dự án khác — trông y hệt một câu trả lời sai bình thường.
    pub project: String,
}

impl SidecarConfig {
    /// Cấu hình chạy service bằng `uv` từ mã nguồn trong repo.
    ///
    /// Đây là đường dùng lúc phát triển. `uv` tự tải Python 3.12 và dựng môi trường ở lần
    /// chạy đầu, nên nó không đòi người dùng cài sẵn gì ngoài chính `uv`.
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

/// Kết nối tới service, mở lười và nối lại được.
pub struct Sidecar {
    config: SidecarConfig,
    /// `AsyncMutex` chứ không phải `parking_lot`: phần dựng kết nối là `async`, và giữ
    /// một khoá đồng bộ qua một `.await` là cách chặn cả runtime.
    running: AsyncMutex<Option<Arc<RunningService<RoleClient, ()>>>>,
    /// Lý do hỏng gần nhất, để `stats()` nói được vì sao thư viện im lặng thay vì chỉ trả
    /// về số không.
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

    /// Gọi một tool, nối lại một lần nếu ống đã đóng.
    pub async fn call(&self, tool: &str, mut args: Map<String, Value>) -> Result<Value, RagError> {
        // Mọi tool của service nhận `project`; điền ở đây thay vì ở bảy chỗ gọi.
        args.insert("project".to_string(), Value::String(self.config.project.clone()));

        match self.call_once(tool, args.clone()).await {
            Ok(value) => {
                *self.last_error.lock() = None;
                Ok(value)
            }
            Err(first) => {
                tracing::debug!(%first, tool, "gọi service hỏng, thử nối lại");
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
            // Server xin hỏi lại người dùng giữa chừng. Service của ta không bao giờ làm
            // vậy — nó không có gì để hỏi — nên đây là một giao thức đã trôi khỏi nhau,
            // và đoán bừa một câu trả lời còn tệ hơn nói ra rằng ta không hiểu.
            other => Err(RagError::Service(format!(
                "`{tool}` trả về phản hồi không mong đợi: {other:?}"
            ))),
        }
    }

    /// Kết nối hiện hành, mở nếu chưa có.
    async fn connect(&self) -> Result<Arc<RunningService<RoleClient, ()>>, RagError> {
        let mut slot = self.running.lock().await;
        if let Some(found) = slot.as_ref() {
            return Ok(found.clone());
        }

        let mut command = tokio::process::Command::new(&self.config.command);
        command.args(&self.config.args);
        // Ép UTF-8 cho tiến trình con **trước** khi áp cấu hình, để cấu hình vẫn đè được.
        //
        // Không có hai dòng này thì trên Windows, stdio của Python mặc định là cp1252 và
        // mọi thông báo lỗi tiếng Việt bị cắt cụt ngay ký tự có dấu đầu tiên: một
        // `ConfigError` dài hai dòng hiện ra thành đúng chữ "kh". Cả tầng lỗi của service
        // được viết để người đọc hành động được, và chuyện đó vô nghĩa nếu chữ không tới
        // được nơi cần đọc.
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("PYTHONUTF8", "1");
        for (key, value) in &self.config.env {
            command.env(key, value);
        }
        if let Some(dir) = &self.config.cwd {
            command.current_dir(dir);
        }

        // `None` cho stderr = để nguyên stderr của tiến trình cha. Xem docstring module.
        let (transport, _stderr) = TokioChildProcess::builder(command)
            .spawn()
            .map_err(|err| {
                RagError::Service(format!(
                    "không chạy được `{}`: {err}. Cài `uv` (https://docs.astral.sh/uv) \
                     hoặc kiểm tra đường dẫn service trong cài đặt.",
                    self.config.command
                ))
            })?;

        // Có trần thời gian vì `uv run` ở lần đầu phải dựng môi trường và tải Python —
        // chậm, nhưng không được phép treo vô hạn. Quá hạn thì nói ra, đừng để giao diện
        // ngồi chờ một thứ sẽ không tới.
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
        tracing::info!(project = %self.config.project, "đã nối tới pai-rag-service");
        Ok(shared)
    }

    /// Bỏ kết nối hiện hành. Lời gọi kế tiếp sẽ mở lại.
    async fn reset(&self) {
        let mut slot = self.running.lock().await;
        *slot = None;
    }

    /// Đóng kết nối và tiến trình con. Gọi lúc tháo plugin.
    pub async fn shutdown(&self) {
        let taken = { self.running.lock().await.take() };
        let Some(service) = taken else { return };
        // Chỉ dừng khi ta là chủ duy nhất còn cầm nó; một lời gọi đang chạy vẫn giữ một
        // `Arc`, và giật ống dưới chân nó là biến một lỗi bình thường thành một panic.
        match Arc::try_unwrap(service) {
            Ok(owned) => {
                if let Err(err) = owned.cancel().await {
                    tracing::debug!(%err, "lỗi lúc đóng pai-rag-service");
                }
            }
            Err(_) => tracing::debug!("còn lời gọi đang chạy, để tiến trình tự đóng"),
        }
    }
}

/// Lần chạy đầu của `uv` phải tải Python và dựng môi trường ảo.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(180);

/// Lấy phần dữ liệu có cấu trúc ra khỏi một `CallToolResult`.
///
/// Đọc `structured_content` chứ không phân tích phần văn bản: phần văn bản là bản JSON
/// tuần tự hoá dành cho người đọc và cho client MCP khác, còn đây là đường máy đọc máy.
/// Bám vào văn bản là bám vào một định dạng không ai hứa giữ nguyên.
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
