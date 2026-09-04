//! One MCP server exposing the whole [`ToolRegistry`].
//! It enforces nothing itself: every call goes through [`ToolPipeline::execute`], the same
//! path the agent loop takes, so hooks, permissions, sandbox and approval apply for free.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pai_tools::{ToolPipeline, ToolRegistry};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde_json::Value;

use crate::naming::is_external;

const INSTRUCTIONS: &str = "Sổ đăng ký tool của Private AI harness. Mọi lời gọi đi qua \
cùng một đường ống canh gác mà agent nội bộ đi, nên một tool có thể trả về lời từ chối \
dưới dạng văn bản thay vì kết quả — đó là câu trả lời hợp lệ, không phải lỗi giao thức.";

/// Exposes the registry; cheap to clone, since the HTTP service builds one per session.
#[derive(Clone)]
pub struct RegistryServer {
    pipeline: Arc<ToolPipeline>,
    /// Numbers calls so each gets its own `call_id`, which the journal and UI use to pair call with result.
    calls: Arc<AtomicU64>,
    relay_external: bool,
}

impl RegistryServer {
    pub fn new(pipeline: Arc<ToolPipeline>) -> RegistryServer {
        RegistryServer {
            pipeline,
            calls: Arc::new(AtomicU64::new(1)),
            relay_external: false,
        }
    }

    /// Whether to relay third-party tools outward; off by default, since a client trusts us, not them.
    pub fn relay_external(mut self, relay: bool) -> RegistryServer {
        self.relay_external = relay;
        self
    }

    fn registry(&self) -> &Arc<ToolRegistry> {
        self.pipeline.registry()
    }
}

impl ServerHandler for RegistryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "private-ai-harness",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    /// The first filter, exactly what the model sees; reuses [`ToolRegistry::schemas`] rather than rebuilding the rules.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .registry()
            .schemas(None)
            .into_iter()
            .filter(|schema| self.relay_external || !is_external(&schema.name))
            .map(|schema| {
                let parameters = match schema.parameters {
                    Value::Object(map) => map,
                    // `ToolSchema` guarantees an object, so this arm is unreachable and beats an `unwrap`.
                    _ => Default::default(),
                };
                // Names go out dotted, as the spec and the registry use them; `__` exists only for the model's wire format.
                Tool::new(
                    schema.name.as_str().to_string(),
                    schema.description,
                    Arc::new(parameters),
                )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    /// The second filter lives in [`ToolPipeline::execute`]; a refusal leaves as `isError: true`, never as a protocol `Err`.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let call_id = format!("mcp-{}", self.calls.fetch_add(1, Ordering::Relaxed));
        let arguments = request.arguments.map_or(Value::Null, Value::Object);
        let outcome = self
            .pipeline
            .execute(&call_id, request.name.as_ref(), arguments)
            .await;

        let content = vec![ContentBlock::text(outcome.content)];
        let mut result = if outcome.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        };
        result.structured_content = outcome.structured;
        Ok(CallToolResponse::Complete(result))
    }
}
