//! Đường ống thi hành, có canh gác.
//!
//! ```text
//! tool/call
//!   → tools/pre-execute   waterfall: hook, quyền, sandbox → allow | deny | ask
//!       ask → phê duyệt, một lần duy nhất; không trả lời được → deny
//!   → guards              đơn điệu: chỉ deny hoặc bỏ qua
//!   → tools/execute       waterfall bao quanh: timeout, retry, đo đạc
//!       → thân tool
//!   → tools/post-execute  waterfall: nhận | chặn | thay | thêm ngữ cảnh
//!   → finalize            đồng bộ, chỉ đụng content
//!   → tools/result        đóng băng, thông báo
//! ```
//!
//! Ba tính chất của cách sắp xếp này đáng được nói thẳng ra, vì cả ba đều là lý do chứ
//! không phải hệ quả:
//!
//! 1. **Canh gác chạy sau phê duyệt.** Người dùng bấm "cho phép" không mở được cái mà
//!    chính sách đã đóng — phê duyệt là điều kiện cần, không phải điều kiện đủ.
//! 2. **Lối bị từ chối vẫn đi qua post-execute.** Một lần từ chối là một sự kiện mà giao
//!    diện và sổ phiên phải thấy, không phải một lối thoát sớm im lặng.
//! 3. **`execute` ở biên ngoài cùng không trả `Result`.** Mọi thứ, kể cả panic và hết
//!    giờ, ra khỏi đây dưới dạng [`ToolOutcome`] đọc được. Một `Err` lọt lên trên chỉ kết
//!    thúc lượt mà không nói gì với mô hình.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Notify, ScopeKey, Waterfall};
use serde_json::{Map, Value, json};

use crate::name::ToolName;
use crate::registry::{Resolution, ToolRegistry};
use crate::schema::ToolMeta;
use crate::seam::{Approval, Elicitation, Spill};
use crate::tool::{Invocation, Tool, ToolOutcome};

/// Bao lâu thì một câu hỏi phê duyệt không được trả lời bị coi là "không".
///
/// Có một thời hạn là bắt buộc: không có nó, một hộp thoại bị che khuất sau cửa sổ khác
/// giữ cả lượt lại vô hạn, và cách người dùng thoát ra sẽ là giết ứng dụng.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// Ngưỡng mặc định trước khi output được cất vào kho tràn.
pub const DEFAULT_SPILL_THRESHOLD: usize = 8_000;

// --- quyết định ------------------------------------------------------------------------

/// Kết quả của `tools/pre-execute`.
#[derive(Clone, Debug, PartialEq)]
pub enum PreDecision {
    Allow,
    /// Lý do đi thẳng tới mô hình, nên nó phải nói được cho mô hình biết phải làm gì khác.
    Deny(String),
    /// Đẩy quyết định cho người dùng.
    Ask {
        reason: String,
    },
}

/// Kết quả của `tools/post-execute`.
///
/// Việc **thay** kết quả không nằm trong enum này mà làm bằng cách sửa `req.outcome`: hai
/// đường cùng mang kết quả thì tầng sau phải chọn tin đường nào, và mọi lựa chọn đều là
/// một chỗ để mất bản sửa của tầng trước.
#[derive(Clone, Debug, PartialEq)]
pub enum PostDecision {
    Accept { additional_context: Vec<String> },
    Block { reason: String },
}

// --- sự kiện ---------------------------------------------------------------------------

pub struct PreRequest {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    pub meta: ToolMeta,
}

pub struct PostRequest {
    pub name: ToolName,
    pub call_id: String,
    pub arguments: Map<String, Value>,
    pub outcome: ToolOutcome,
}

/// Kết quả đã đóng băng.
pub struct ResolvedCall {
    pub name: ToolName,
    pub call_id: String,
    pub outcome: ToolOutcome,
    pub additional_context: Vec<String>,
}

pub enum PreExecute {}
impl Waterfall for PreExecute {
    const NAME: &'static str = "tools/pre-execute";
    type Req = PreRequest;
    type Out = PreDecision;
}

pub enum Execute {}
impl Waterfall for Execute {
    const NAME: &'static str = "tools/execute";
    type Req = Invocation;
    type Out = ToolOutcome;
}

pub enum PostExecute {}
impl Waterfall for PostExecute {
    const NAME: &'static str = "tools/post-execute";
    type Req = PostRequest;
    type Out = PostDecision;
}

pub enum ToolResult {}
impl Notify for ToolResult {
    const NAME: &'static str = "tools/result";
    type Payload = ResolvedCall;
}

// --- canh gác --------------------------------------------------------------------------

/// Canh gác đơn điệu.
///
/// **Không có nhánh cho phép, và đó là toàn bộ ý nghĩa của trait này.** Một canh gác trả
/// `Some(lý do)` để từ chối, hoặc `None` để bỏ qua. Vì không ai nói "cho phép" được, thứ
/// tự đăng ký không thể biến một lệnh từ chối thành cho phép: kết quả là phép **hoặc**
/// của các lời từ chối, mà phép hoặc thì giao hoán.
///
/// Đây là chỗ để chính sách của chủ sở hữu — cái không được phép bị sắp xếp lại. Chính
/// sách thương lượng được thì cắm vào `tools/pre-execute`, nơi có cả ba nhánh.
#[async_trait]
pub trait ToolGuard: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    /// `Some(lý do)` = từ chối. `None` = không có ý kiến.
    async fn check(&self, call: &Invocation, meta: &ToolMeta) -> Option<String>;
}

// --- phê duyệt -------------------------------------------------------------------------

pub struct ApprovalRequest {
    pub name: ToolName,
    pub call_id: String,
    pub reason: String,
    pub arguments: Map<String, Value>,
    pub meta: ToolMeta,
}

/// Hỏi người dùng cho phép, **một lần cho một lần gọi**.
///
/// Trait cố tình chỉ trả `bool` chứ không có biến thể "nhớ lựa chọn": một câu trả lời
/// được ghi nhớ là một câu trả lời cho một câu hỏi mà người dùng chưa nghe. Ngữ cảnh của
/// lần gọi sau khác lần gọi này — đường dẫn khác, nội dung khác, và có thể là một tài
/// liệu vừa được nạp vào đang cố lái mô hình. Muốn bớt hỏi thì nới chính sách ở
/// `tools/pre-execute`, nơi việc nới được viết ra và đọc lại được.
#[async_trait]
pub trait Approver: Send + Sync + 'static {
    async fn approve(&self, request: &ApprovalRequest) -> bool;
}

// --- đường ống -------------------------------------------------------------------------

/// Văn bản mô hình đọc khi nó gọi một tool nó không được gọi.
///
/// Cùng một câu cho "bị cấm" và cho "không tồn tại", cố ý: hai câu khác nhau biến hàm gọi
/// thành một máy dò, cho phép mô hình liệt kê những tool bị giấu bằng cách đoán tên. Sự
/// khác biệt vẫn được giữ, nhưng ở `meta` — chỗ chỉ host đọc.
pub fn not_available(name: &ToolName) -> String {
    format!("Tool `{name}` không khả dụng với agent này.")
}

/// Ép một closure vào đúng ràng buộc higher-ranked mà `Context::waterfall` đòi ở tail.
///
/// Không có nó, trình suy diễn gán cho closure một lifetime cụ thể thay vì `for<'r>`, và
/// lỗi báo ra là một dòng "one type is more general than the other" chẳng chỉ vào đâu cả.
fn tail<E: Waterfall, F>(f: F) -> F
where
    F: for<'r> Fn(&'r mut E::Req) -> BoxFuture<'r, E::Out> + Send + Sync,
{
    f
}

pub struct ToolPipeline {
    ctx: Context,
    registry: Arc<ToolRegistry>,
    scope: Option<ScopeKey>,
    spill_threshold: usize,
    approval_timeout: Duration,
}

impl ToolPipeline {
    /// Phạm vi lấy từ chính `ctx`: một đường ống dựng trong ngữ cảnh của agent nào thì
    /// chịu hạn chế của agent đó, không có cách nào dựng nhầm.
    pub fn new(ctx: &Context, registry: Arc<ToolRegistry>) -> ToolPipeline {
        ToolPipeline {
            scope: ctx.scope_key(),
            ctx: ctx.clone(),
            registry,
            spill_threshold: DEFAULT_SPILL_THRESHOLD,
            approval_timeout: APPROVAL_TIMEOUT,
        }
    }

    pub fn with_spill_threshold(mut self, chars: usize) -> ToolPipeline {
        self.spill_threshold = chars;
        self
    }

    pub fn with_approval_timeout(mut self, timeout: Duration) -> ToolPipeline {
        self.approval_timeout = timeout;
        self
    }

    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Biên ngoài cùng. **Không trả `Result`** — xem ghi chú đầu module.
    pub async fn execute(&self, call_id: &str, raw_name: &str, arguments: Value) -> ToolOutcome {
        // Tầng lọc thứ hai. Chạy trên tên đã giải mã, trước khi chạm vào bất cứ thứ gì
        // thuộc về tool: một mô hình đoán ra tên trên wire dừng lại đúng ở dòng này.
        let (tool, name) = match self.registry.resolve(self.scope, raw_name) {
            Resolution::Found(tool, name) => (tool, name),
            Resolution::Denied(name) => {
                return ToolOutcome::error(not_available(&name))
                    .with_meta("refusal", json!("denied"))
                    .with_meta("tool", json!(name.as_str()));
            }
            Resolution::Unknown(name) => {
                return ToolOutcome::error(not_available(&name))
                    .with_meta("refusal", json!("unknown"))
                    .with_meta("tool", json!(name.as_str()));
            }
        };

        let meta = tool.meta();
        let mut args = match arguments {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            _ => {
                return ToolOutcome::error(format!("Tool `{name}` cần tham số dạng object JSON."));
            }
        };
        self.registry.apply_pins(self.scope, &mut args);

        let denial = self.gate(&name, call_id, &meta, &mut args).await;

        let mut inv = Invocation::new(name.clone(), call_id, args)
            .with_elicitor(self.ctx.get::<Elicitation>());

        let outcome = match denial {
            Some(text) => ToolOutcome::error(text).with_meta("refusal", json!("policy")),
            None => self.run(&tool, &meta, &mut inv).await,
        };

        self.settle(tool.as_ref(), name, call_id, inv.arguments, outcome)
            .await
    }

    /// pre-execute → phê duyệt → canh gác. `Some(lý do)` nghĩa là thân tool không chạy.
    async fn gate(
        &self,
        name: &ToolName,
        call_id: &str,
        meta: &ToolMeta,
        args: &mut Map<String, Value>,
    ) -> Option<String> {
        let mut pre = PreRequest {
            name: name.clone(),
            call_id: call_id.to_string(),
            arguments: std::mem::take(args),
            meta: meta.clone(),
        };
        let decision = self
            .ctx
            .waterfall::<PreExecute, _>(&mut pre, |_| async { PreDecision::Allow }.boxed())
            .await;

        *args = std::mem::take(&mut pre.arguments);
        // Ghim lại sau waterfall: một middleware được phép sửa tham số, nhưng không được
        // phép gỡ ghim. Nếu bỏ dòng này thì một hook vô hại trở thành đường vòng qua đúng
        // cái ràng buộc mà ghim tồn tại để giữ.
        self.registry.apply_pins(self.scope, args);

        match decision {
            PreDecision::Deny(reason) => return Some(reason),
            PreDecision::Ask { reason } => {
                let request = ApprovalRequest {
                    name: name.clone(),
                    call_id: call_id.to_string(),
                    reason: reason.clone(),
                    arguments: args.clone(),
                    meta: meta.clone(),
                };
                if !self.ask(&request).await {
                    return Some(format!("Người dùng không cho phép `{name}`: {reason}"));
                }
            }
            PreDecision::Allow => {}
        }

        // Canh gác chạy **sau** phê duyệt: một cái "cho phép" của người dùng không mở
        // được cái mà chính sách đã đóng.
        let probe = Invocation::new(name.clone(), call_id, args.clone());
        for guard in self.registry.guards(self.scope) {
            let checking = AssertUnwindSafe(guard.check(&probe, meta)).catch_unwind();
            match checking.await {
                Ok(None) => {}
                Ok(Some(reason)) => {
                    // Dừng ở lời từ chối đầu tiên. Chạy nốt phần còn lại không đổi được
                    // câu trả lời, vì không canh gác nào nói "cho phép" được.
                    tracing::info!(tool = %name, guard = guard.name(), "canh gác từ chối");
                    return Some(reason);
                }
                Err(_) => {
                    // Một canh gác hoảng loạn là một canh gác không kết luận được, và
                    // "không kết luận được" phải nghiêng về phía từ chối.
                    tracing::error!(tool = %name, guard = guard.name(), "canh gác hoảng loạn");
                    return Some(format!(
                        "Canh gác `{}` không kiểm tra được `{name}`, nên lệnh gọi bị từ chối.",
                        guard.name()
                    ));
                }
            }
        }
        None
    }

    /// Hỏi phê duyệt. Fail-closed ở cả ba nhánh: không có ai để hỏi, hết giờ, hoặc bên
    /// hỏi hoảng loạn — cả ba đều là "không".
    async fn ask(&self, request: &ApprovalRequest) -> bool {
        let Some(approver) = self.ctx.get::<Approval>() else {
            tracing::warn!(tool = %request.name, "không có approver nào cắm vào: từ chối");
            return false;
        };
        let asking = AssertUnwindSafe(approver.approve(request)).catch_unwind();
        match tokio::time::timeout(self.approval_timeout, asking).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => {
                tracing::error!(tool = %request.name, "approver hoảng loạn: từ chối");
                false
            }
            Err(_) => {
                tracing::warn!(tool = %request.name, "phê duyệt hết giờ: từ chối");
                false
            }
        }
    }

    /// `tools/execute`, có timeout bao quanh.
    async fn run(
        &self,
        tool: &Arc<dyn Tool>,
        meta: &ToolMeta,
        inv: &mut Invocation,
    ) -> ToolOutcome {
        let cancel = inv.cancel_token();
        let body = tail::<Execute, _>(move |call: &mut Invocation| {
            let tool = tool.clone();
            async move {
                match tool.execute(&*call).await {
                    Ok(outcome) => outcome,
                    // Thân tool được phép trả `Err`; ra tới đây thì nó thành văn bản.
                    Err(err) => ToolOutcome::error(format!("Tool `{}` lỗi: {err}", call.name)),
                }
            }
            .boxed()
        });

        let finished = {
            let running = self.ctx.waterfall::<Execute, _>(inv, body);
            tokio::time::timeout(meta.timeout, AssertUnwindSafe(running).catch_unwind()).await
        };

        match finished {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                ToolOutcome::error(format!("Tool `{}` hoảng loạn và bị dừng lại.", inv.name))
                    .with_meta("failure", json!("panic"))
            }
            Err(_) => {
                // Huỷ để thân tool bỏ việc dở thay vì chạy tiếp trong nền sau khi kết quả
                // của nó đã không còn ai nhận.
                cancel.cancel();
                ToolOutcome::error(format!(
                    "Tool `{}` quá {} giây và bị dừng lại.",
                    inv.name,
                    meta.timeout.as_secs()
                ))
                .with_meta("failure", json!("timeout"))
            }
        }
    }

    /// post-execute → finalize → tràn → thông báo.
    async fn settle(
        &self,
        tool: &dyn Tool,
        name: ToolName,
        call_id: &str,
        arguments: Map<String, Value>,
        outcome: ToolOutcome,
    ) -> ToolOutcome {
        let mut post = PostRequest {
            name: name.clone(),
            call_id: call_id.to_string(),
            arguments,
            outcome,
        };
        let decision = self
            .ctx
            .waterfall::<PostExecute, _>(&mut post, |_| {
                async {
                    PostDecision::Accept {
                        additional_context: Vec::new(),
                    }
                }
                .boxed()
            })
            .await;

        let mut outcome = post.outcome;
        let additional = match decision {
            PostDecision::Accept { additional_context } => additional_context,
            PostDecision::Block { reason } => {
                outcome = ToolOutcome::error(reason).with_meta("refusal", json!("post"));
                Vec::new()
            }
        };

        // Đồng bộ, chỉ đụng content — tool không lật được `is_error` sau khi chính sách
        // đã chạy xong.
        let mut content = std::mem::take(&mut outcome.content);
        if std::panic::catch_unwind(AssertUnwindSafe(|| tool.finalize(&mut content))).is_err() {
            tracing::error!(tool = %name, "finalize hoảng loạn; giữ nguyên content");
        }
        outcome.content = content;

        self.spill(&name, &mut outcome);

        if !additional.is_empty() {
            outcome
                .meta
                .insert("additional_context".into(), json!(additional.clone()));
        }

        // Đóng băng: từ đây trở đi kết quả chỉ được đọc.
        self.ctx.notify::<ToolResult>(&ResolvedCall {
            name,
            call_id: call_id.to_string(),
            outcome: outcome.clone(),
            additional_context: additional,
        });
        outcome
    }

    /// Cất phần dư vào kho thay vì cắt bỏ nó.
    fn spill(&self, name: &ToolName, outcome: &mut ToolOutcome) {
        if outcome.content.chars().count() <= self.spill_threshold {
            return;
        }
        let Some(store) = self.ctx.get::<Spill>() else {
            // Không có kho thì gửi nguyên văn. Dài còn sửa được; mất thì không.
            tracing::warn!(tool = %name, "output vượt ngưỡng nhưng chưa có kho tràn nào cắm vào");
            return;
        };
        let full = std::mem::take(&mut outcome.content);
        let handle = store.spill(name, &full);
        let head: String = full.chars().take(self.spill_threshold).collect();
        let rest = handle.chars.saturating_sub(self.spill_threshold);
        outcome.content = format!(
            "{head}\n\n[… còn {rest} ký tự. Toàn văn được giữ nguyên vẹn tại `{}` \
             ({} dòng); không có gì bị cắt bỏ.]",
            handle.id, handle.lines
        );
        outcome.meta.insert("spill".into(), handle.to_json());
    }
}
