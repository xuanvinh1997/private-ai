//! This crate's seams: who asks for approval, who asks for a value, and where spill goes.
//! All three are looked up on `Context` at call time rather than cached, so unmounting the
//! approval dialog turns later questions into refusals instead of stale passes.

use pai_core::ServiceKey;

use crate::pipeline::Approver;
use crate::registry::ToolRegistry;
use crate::spill::SpillStore;
use crate::tool::Elicitor;

/// The tool registry.
pub enum Tools {}
impl ServiceKey for Tools {
    type Api = ToolRegistry;
    const NAME: &'static str = "tools";
}

/// Ask for approval; no provider means everything that needs asking is refused.
pub enum Approval {}
impl ServiceKey for Approval {
    type Api = dyn Approver;
    const NAME: &'static str = "tools/approval";
}

/// Ask for a value; no provider means it cannot be asked.
pub enum Elicitation {}
impl ServiceKey for Elicitation {
    type Api = dyn Elicitor;
    const NAME: &'static str = "tools/elicitation";
}

/// The spill store; no provider means nothing is trimmed and long output passes whole.
pub enum Spill {}
impl ServiceKey for Spill {
    type Api = dyn SpillStore;
    const NAME: &'static str = "tools/spill";
}
