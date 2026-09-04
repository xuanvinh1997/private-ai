//! Scoped tool registry and a guarded execution pipeline: the product's security floor.
//! Permissions are checked at listing and again at call time, pinned parameters leave the
//! schema entirely, refusals are text not errors, and guards can only deny, never allow.

pub mod budget;
pub mod builtin;
pub mod name;
pub mod pipeline;
pub mod plugin;
pub mod registry;
pub mod schema;
pub mod seam;
pub mod spill;
pub mod tool;

pub use budget::{BYTES_PER_TOKEN, DEFAULT_TOKEN_BUDGET, Folded, Overflow, Split, approx_tokens};
pub use name::{ToolName, WIRE_SEPARATOR};
pub use pipeline::{
    APPROVAL_TIMEOUT, ApprovalRequest, Approver, Execute, PostDecision, PostExecute, PostRequest,
    PreDecision, PreExecute, PreRequest, ResolvedCall, ToolGuard, ToolPipeline, ToolResult,
    not_available,
};
pub use plugin::ToolsPlugin;
pub use registry::{Resolution, ToolRegistry, ToolRestriction};
pub use schema::{ToolMeta, ToolSchema, UNTRUSTED_NOTICE, json_schema_for};
pub use seam::{Approval, Elicitation, Spill, Tools};
pub use spill::{MemorySpillStore, SpillRef, SpillStore};
pub use tool::{Elicitor, Invocation, Tool, ToolError, ToolOutcome};
