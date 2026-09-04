//! A tool on another server, wearing the shape of a local [`Tool`].
//! This is where the trust boundary is written down as code. See [`RemoteTool::meta`].

use std::sync::Arc;

use async_trait::async_trait;
use pai_tools::{Invocation, Tool, ToolError, ToolMeta, ToolName, ToolOutcome, ToolSchema};
use parking_lot::RwLock;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock};
use rmcp::service::{Peer, RoleClient};
use serde_json::{Value, json};

use crate::dial::Reach;

/// Holds a server's current connection; tools keep an `Arc<Link>`, not a [`Peer`], so a reconnect leaves the tool list intact.
#[derive(Default)]
pub struct Link {
    peer: RwLock<Option<Peer<RoleClient>>>,
}

impl Link {
    pub fn new() -> Arc<Link> {
        Arc::new(Link::default())
    }

    pub fn set(&self, peer: Peer<RoleClient>) {
        *self.peer.write() = Some(peer);
    }

    pub fn clear(&self) {
        *self.peer.write() = None;
    }

    pub fn peer(&self) -> Option<Peer<RoleClient>> {
        self.peer.read().clone()
    }

    pub fn connected(&self) -> bool {
        self.peer.read().is_some()
    }
}

/// A tool someone else wrote, called over MCP.
pub struct RemoteTool {
    /// Already prefixed `ext.<server>.` — see [`crate::naming`].
    name: ToolName,
    /// The bare name as published, kept rather than re-derived from `name`, so stripping happens in one place.
    remote: String,
    server: String,
    description: String,
    parameters: Value,
    link: Arc<Link>,
    reach: Reach,
}

impl RemoteTool {
    pub fn new(
        name: ToolName,
        remote: impl Into<String>,
        server: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        link: Arc<Link>,
        reach: Reach,
    ) -> RemoteTool {
        RemoteTool {
            name,
            remote: remote.into(),
            server: server.into(),
            description: description.into(),
            parameters,
            link,
            reach,
        }
    }

    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// Fold a `CallToolResult` into what the pipeline understands; non-text blocks become a line saying what was there.
    fn render(result: CallToolResult) -> ToolOutcome {
        let mut parts: Vec<String> = Vec::new();
        for block in &result.content {
            match block {
                ContentBlock::Text(text) => parts.push(text.text.clone()),
                ContentBlock::Image(image) => {
                    parts.push(format!("[ảnh {} bị lược bỏ]", image.mime_type));
                }
                ContentBlock::Audio(audio) => {
                    parts.push(format!("[âm thanh {} bị lược bỏ]", audio.mime_type));
                }
                ContentBlock::Resource(_) => parts.push("[tài nguyên nhúng bị lược bỏ]".into()),
                ContentBlock::ResourceLink(link) => {
                    parts.push(format!("[liên kết tài nguyên: {}]", link.uri));
                }
                _ => parts.push("[khối nội dung không nhận ra]".into()),
            }
        }
        let mut outcome = if result.is_error.unwrap_or(false) {
            ToolOutcome::error(parts.join("\n"))
        } else {
            ToolOutcome::ok(parts.join("\n"))
        };
        if let Some(structured) = result.structured_content {
            outcome = outcome.with_structured(structured);
        }
        outcome
    }
}

#[async_trait]
impl Tool for RemoteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.name.clone(),
            self.description.clone(),
            self.parameters.clone(),
        )
    }

    /// Worst-case assumptions: `readOnlyHint` is only a hint, the text is written by strangers, and concurrency is unproven.
    /// Only `leaves_device`, taken from [`Reach`], is something we actually know.
    fn meta(&self) -> ToolMeta {
        ToolMeta {
            mutating: true,
            leaves_device: self.reach.leaves_device(),
            returns_untrusted_content: true,
            concurrency_safe: false,
            ..ToolMeta::default()
        }
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let Some(peer) = self.link.peer() else {
            return Err(ToolError::Failed(format!(
                "MCP server `{}` đang không kết nối.",
                self.server
            )));
        };

        let mut params = CallToolRequestParams::new(self.remote.clone());
        if !call.arguments.is_empty() {
            params = params.with_arguments(call.arguments.clone());
        }

        // A pipeline timeout cancels this token; unwatched, the remote call keeps running after the result is dropped.
        let cancelled = call.cancel_token();
        let response = tokio::select! {
            () = cancelled.cancelled() => {
                return Err(ToolError::Failed(format!(
                    "Lời gọi tới `{}` bị huỷ trước khi server trả lời.", self.name
                )));
            }
            response = peer.call_tool_once(params) => response,
        };

        match response {
            Ok(CallToolResponse::Complete(result)) => Ok(Self::render(result)
                .with_meta("mcp_server", json!(self.server))
                .with_meta("mcp_tool", json!(self.remote))),
            // The multi-step negotiation rounds are not wired to approval, and auto-accepting a third party's request is worse.
            Ok(CallToolResponse::InputRequired(_)) => Err(ToolError::Failed(format!(
                "Server `{}` xin thêm đầu vào giữa chừng; harness chưa hỗ trợ vòng đó.",
                self.server
            ))),
            Ok(CallToolResponse::Task(_)) => Err(ToolError::Failed(format!(
                "Server `{}` trả về một task chạy nền; harness chưa hỗ trợ.",
                self.server
            ))),
            Err(err) => Err(ToolError::Failed(format!(
                "Server `{}` báo lỗi: {err}",
                self.server
            ))),
            // `CallToolResponse` is non_exhaustive; for an unknown variant, saying so beats guessing what it means.
            Ok(_) => Err(ToolError::Failed(format!(
                "Server `{}` trả về một loại kết quả harness chưa biết đọc.",
                self.server
            ))),
        }
    }
}
