//! The server half: a real MCP client talking to [`RegistryServer`] over an in-memory pipe.
//! Locks two sentences: what is exposed is the registry itself, not a copy of it, and a
//! refusal leaves as text rather than as a protocol error.

use std::sync::Arc;

use async_trait::async_trait;
use pai_core::Context;
use pai_mcp::RegistryServer;
use pai_tools::pipeline::ToolGuard;
use pai_tools::{
    Invocation, Tool, ToolError, ToolMeta, ToolName, ToolOutcome, ToolPipeline, ToolRegistry,
    ToolSchema,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use serde_json::json;

struct Builtin(ToolName);

#[async_trait]
impl Tool for Builtin {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            self.0.clone(),
            "tool nội bộ",
            json!({ "type": "object", "properties": {} }),
        )
    }

    fn meta(&self) -> ToolMeta {
        ToolMeta::read_only()
    }

    async fn execute(&self, _call: &Invocation) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::ok("nội bộ đã chạy"))
    }
}

/// A monotonous guard: it only refuses, never allows.
struct NoBash;

#[async_trait]
impl ToolGuard for NoBash {
    fn name(&self) -> &'static str {
        "no-bash"
    }

    async fn check(&self, call: &Invocation, _meta: &ToolMeta) -> Option<String> {
        (call.name.as_str() == "bash").then(|| "bash bị chính sách chặn".to_string())
    }
}

/// Connect a real MCP client to an in-process `RegistryServer`.
async fn connect(server: RegistryServer) -> RunningService<RoleClient, ()> {
    let (client_side, server_side) = tokio::io::duplex(8 * 1024);
    tokio::spawn(async move {
        if let Ok(service) = server.serve(server_side).await {
            let _ = service.waiting().await;
        }
    });
    ().serve(client_side).await.expect("client nối được")
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The exposed list matches the registry exactly, in dotted form.
#[tokio::test]
async fn phoi_dung_cai_so_dang_ky_noi() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let keep = registry.register(Arc::new(Builtin(ToolName::new("fs.read"))));
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry.clone()));

    let client = connect(RegistryServer::new(pipeline)).await;
    let tools = client.peer().list_all_tools().await.expect("liệt kê được");
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();

    assert_eq!(names, vec!["fs.read".to_string()]);
    let _ = client.cancel().await;
    drop(keep);
}

/// Third-party tools are not re-exposed unless someone says so: a client here trusts us, not them.
#[tokio::test]
async fn khong_phoi_lai_tool_ben_thu_ba() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let keep_own = registry.register(Arc::new(Builtin(ToolName::new("read"))));
    let keep_ext = registry.register(Arc::new(Builtin(ToolName::new("ext.other.thing"))));
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry.clone()));

    let client = connect(RegistryServer::new(pipeline.clone())).await;
    let names: Vec<String> = client
        .peer()
        .list_all_tools()
        .await
        .expect("liệt kê được")
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(names, vec!["read".to_string()]);
    let _ = client.cancel().await;

    // Turned on explicitly it appears, and then it is the operator's decision.
    let client = connect(RegistryServer::new(pipeline).relay_external(true)).await;
    let names: Vec<String> = client
        .peer()
        .list_all_tools()
        .await
        .expect("liệt kê được")
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"ext.other.thing".to_string()));
    let _ = client.cancel().await;

    drop((keep_own, keep_ext));
}

/// Calls really work, and go through the pipeline rather than straight into the tool body.
#[tokio::test]
async fn goi_duoc_va_di_qua_duong_ong() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let keep = registry.register(Arc::new(Builtin(ToolName::new("read"))));
    let keep_bash = registry.register(Arc::new(Builtin(ToolName::new("bash"))));
    let keep_guard = registry.add_guard(None, Arc::new(NoBash));
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry.clone()));

    let client = connect(RegistryServer::new(pipeline)).await;

    let ok = client
        .call_tool(CallToolRequestParams::new("read"))
        .await
        .expect("gọi được");
    assert_eq!(ok.is_error, Some(false));
    assert!(text_of(&ok).contains("nội bộ đã chạy"));

    // The pipeline's guard applies to outside clients too, not because the server remembers to check but because it never does.
    let blocked = client
        .call_tool(CallToolRequestParams::new("bash"))
        .await
        .expect("vẫn nhận được câu trả lời");
    assert_eq!(blocked.is_error, Some(true));
    assert!(text_of(&blocked).contains("bash bị chính sách chặn"));

    let _ = client.cancel().await;
    drop((keep, keep_bash, keep_guard));
}

/// A refusal is text, not a protocol error: an `McpError` reads as "we broke" and gets retried the same way.
#[tokio::test]
async fn tu_choi_ra_ngoai_duoi_dang_van_ban() {
    let ctx = Context::root();
    let registry = ToolRegistry::new(&ctx);
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry));

    let client = connect(RegistryServer::new(pipeline)).await;
    let result = client
        .call_tool(CallToolRequestParams::new("khong_ton_tai"))
        .await
        .expect("phải là một kết quả, không phải một lỗi giao thức");

    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("không khả dụng"));
    let _ = client.cancel().await;
}
