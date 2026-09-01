//! Một server MCP duy nhất, phơi cả [`ToolRegistry`] ra ngoài.
//!
//! Bản Python có **tám** console-script, mỗi cái một tập tool. Con số đó không đến từ một
//! ranh giới nào cả: nó đến từ việc mỗi nhóm tool được viết vào một lúc khác nhau. Cái giá
//! là tám tiến trình phải nuôi, tám chỗ để cấu hình lệch nhau, và một câu hỏi không có câu
//! trả lời — quyền được kiểm ở đâu khi mỗi server có một bản sao của luật.
//!
//! Ở đây có **một**, và nó không tự kiểm gì cả: mọi lời gọi đi qua
//! [`ToolPipeline::execute`], đúng cái đường mà vòng lặp agent đi. Hook, quyền, sandbox,
//! phê duyệt, canh gác — tất cả tự động áp cho client bên ngoài, vì chúng nằm trên đường
//! ống chứ không nằm trong người gọi. Thêm một luật mới không phải sửa tệp này.

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

/// Phơi sổ đăng ký ra ngoài.
///
/// Rẻ để clone: [`rmcp::transport::StreamableHttpService`] dựng một bản cho mỗi phiên.
#[derive(Clone)]
pub struct RegistryServer {
    pipeline: Arc<ToolPipeline>,
    /// Đánh số lời gọi để mỗi lần có một `call_id` riêng. Sổ phiên và giao diện dùng nó
    /// để ghép lời gọi với kết quả, nên nó phải là duy nhất trong một tiến trình.
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

    /// Có phơi lại tool của server bên thứ ba ra ngoài không. Mặc định **không**.
    ///
    /// Một client nối vào server này tin *ta*. Chuyển tiếp tool của người thứ ba dưới cái
    /// tên của ta là cho mượn lòng tin đó cho một bên mà client kia chưa bao giờ chọn, và
    /// hai harness trỏ vào nhau thì thành một vòng lặp không ai nhìn thấy đáy. Ai thật sự
    /// muốn thì bật, và lúc đó đó là quyết định của họ, viết ra ở chỗ đọc lại được.
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

    /// Tầng lọc thứ nhất, y hệt cái mô hình nhìn thấy.
    ///
    /// Dùng thẳng [`ToolRegistry::schemas`] chứ không tự ráp danh sách: hàm đó đã lọc
    /// quyền, đã chèn khung cảnh báo nội dung không đáng tin, và đã giấu tham số ghim.
    /// Ráp lại ở đây là dựng một bản sao của luật, rồi để nó trôi.
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
                    // `ToolSchema` bảo đảm đây luôn là object; nhánh này không với tới
                    // được, và một schema rỗng vẫn tốt hơn một lần `unwrap`.
                    _ => Default::default(),
                };
                // Tên đi ra ở **dạng chuẩn có dấu chấm** — đúng dạng mà spec cho phép và
                // đúng dạng mà sổ đăng ký nói. Dạng `__` chỉ tồn tại cho wire format của
                // mô hình, và mang nó ra đây là để lộ chi tiết của một giao thức khác.
                Tool::new(
                    schema.name.as_str().to_string(),
                    schema.description,
                    Arc::new(parameters),
                )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    /// Tầng lọc thứ hai nằm bên trong [`ToolPipeline::execute`], không nằm ở đây.
    ///
    /// Và một luật nữa: **từ chối không bao giờ ra ngoài dưới dạng `Err`**. Một `McpError`
    /// là lỗi *giao thức* — client bên kia sẽ coi đó là ta hỏng, không phải là nó không
    /// được phép, và mô hình phía nó không đọc được lý do. Từ chối đi ra dưới dạng
    /// `isError: true` kèm câu chữ, đúng như ở phía trong.
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
