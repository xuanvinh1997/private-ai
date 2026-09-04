//! A single turn's tool scope, translated into registry restrictions. The scope is chosen in the composer and
//! means only that turn, so it lives in a per-turn child scope whose disposal restores full rights.
//! Restricting goes through [`ToolRegistry::restrict`], which gates both listing and lookup with one check.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use pai_agent::AgentRequest;
use pai_core::{Context, Middleware, Next, ScopeKey};
use pai_llm::{ChatRequest, Message};
use pai_tools::{ToolRegistry, ToolRestriction, Tools};

use crate::protocol::ToolScope;

/// Tools that execute commands on this machine -- the boundary between `write` and `shell`. Known debt:
/// `ToolMeta` has no "runs arbitrary commands" flag, so this list is hand-written and fails open.
/// `task` belongs here because restrictions do not inherit into child scopes, making it a back door to `bash`.
pub const TOOL_THI_HANH: &[&str] = &[
    "bash",
    "job_kill",
    "task",
    "terminal_open",
    "terminal_send",
    "terminal_signal",
    "terminal_close",
];

/// The restriction for a scope, `None` meaning unrestricted; the tool set is read from `at` rather than a
/// hard-coded name list, so a newly registered tool lands in the right group by itself.
pub fn han_che(
    registry: &ToolRegistry,
    at: Option<ScopeKey>,
    scope: ToolScope,
) -> Option<ToolRestriction> {
    match scope {
        // No restriction at all: the only such case here, and the user must choose it explicitly.
        ToolScope::Shell => None,
        // An allowlist built from `ToolMeta::mutating`, which defaults to `true`, so an undeclared tool falls outside read-only -- fail closed.
        ToolScope::Read => Some(ToolRestriction::allow_only(
            registry
                .visible(at)
                .into_iter()
                .filter(|tool| !tool.meta().mutating)
                .map(|tool| tool.schema().name),
        )),
        // A denylist, because this direction cannot be derived from `ToolMeta` -- see [`TOOL_THI_HANH`].
        ToolScope::Write => Some(ToolRestriction::deny_only(TOOL_THI_HANH.iter().copied())),
    }
}

/// The sentence telling the model it is restricted, or `None` when it is not. Saying so matters: otherwise the
/// only signal is a refusal that reads like a permanent property of the agent, and the model either thrashes or
/// reports success it never achieved. It names the level only, never the hidden tools, so it cannot be probed.
pub fn loi_nhac(scope: ToolScope) -> Option<String> {
    match scope {
        ToolScope::Shell => None,
        ToolScope::Read => Some(
            "Người dùng đặt lượt này ở phạm vi **chỉ đọc**: chỉ những tool không thay đổi \
             gì mới được cắm. Tool sửa tệp và tool chạy lệnh không có ở lượt này — đừng \
             thử gọi chúng. Cần tới chúng thì mô tả việc phải làm và nói người dùng nâng \
             phạm vi lên, thay vì báo là đã làm xong."
                .into(),
        ),
        ToolScope::Write => Some(
            "Người dùng đặt lượt này ở phạm vi **đọc và ghi**: đọc và sửa tệp thì được, \
             chạy lệnh và giao việc cho agent con thì không. Cần chạy lệnh thì nói ra \
             lệnh cần chạy và để người dùng nâng phạm vi lên, thay vì báo là đã chạy."
                .into(),
        ),
    }
}

/// Append [`loi_nhac`] to the system message of every request in the turn, via `agent/request` rather than
/// `SystemPrompt`, which is app-wide; this binds to the turn's scope and never enters the session log.
struct NhacPhamVi(String);

impl Middleware<AgentRequest> for NhacPhamVi {
    fn call<'a>(
        &'a self,
        req: &'a mut ChatRequest,
        next: Next<'a, AgentRequest>,
    ) -> BoxFuture<'a, ChatRequest> {
        match req.messages.first_mut() {
            // Append to the existing system block, so the turn's rule sits next to the general ones instead of in a message compaction may drop.
            Some(Message::System { content }) => {
                content.push_str("\n\n");
                content.push_str(&self.0);
            }
            _ => req.messages.insert(0, Message::system(self.0.clone())),
        }
        next.run(req).boxed()
    }
}

/// Open a child scope for exactly one turn with its restriction installed; the caller must `dispose()` it when
/// the turn ends, since that is when rights are restored.
pub fn mo_pham_vi(
    ctx: &Context,
    scope: ToolScope,
    approver: Arc<dyn pai_tools::Approver>,
) -> Result<Context, String> {
    // Fetch the registry before creating the scope: failing here leaves nothing to clean up.
    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().map_err(|err| err.to_string())?;
    // `isolate` before `scoped`: each turn's approver needs its own realm, or two concurrent turns collide on
    // the same seam and the second one cannot start.
    let turn = ctx.isolate::<pai_tools::Approval>().scoped("luot");
    // If the scope cannot be created, refuse to run rather than run unrestricted.
    let key = turn
        .scope_key()
        .ok_or("không dựng được phạm vi riêng cho lượt")?;
    if let Some(restriction) = han_che(&registry, Some(key), scope) {
        turn.keep(registry.restrict(key, restriction));
    }
    if let Some(text) = loi_nhac(scope) {
        turn.keep(turn.on_waterfall::<AgentRequest>(Arc::new(NhacPhamVi(text))));
    }
    // The approver goes in the turn's own realm because it holds that turn's `Channel`; at the root, two
    // concurrent turns would prompt into each other's windows.
    turn.keep(
        turn.provide::<pai_tools::Approval>(approver)
            .map_err(|err| err.to_string())?,
    );
    Ok(turn)
}
