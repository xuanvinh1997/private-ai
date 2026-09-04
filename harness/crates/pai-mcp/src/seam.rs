//! This crate's seams: the hub (servers running) and the store (servers the user declared).
//! The server side has no seam because it is a gate, not a capability anyone consumes: it
//! is either open or not, never swapped for another implementation.

use pai_core::ServiceKey;

use crate::hub::McpHub;
use crate::store::McpStore;

/// Every third-party server; no provider means no external tools and everything else still runs.
pub enum Mcp {}
impl ServiceKey for Mcp {
    type Api = McpHub;
    const NAME: &'static str = "mcp";
}

/// The user's own server list; separate from [`Mcp`] because the hub says what runs and this says what should.
pub enum McpConfig {}
impl ServiceKey for McpConfig {
    type Api = McpStore;
    const NAME: &'static str = "mcp.store";
}
