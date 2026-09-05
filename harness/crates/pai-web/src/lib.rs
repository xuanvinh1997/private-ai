//! The web layer: the first native crate that leaves this device.
//!
//! Until now everything that reached the network did so through `pai-mcp`, which declares
//! `leaves_device` on a tool's behalf from the transport it was dialled over. Native tools have no
//! transport to be asked, so [`crate::tools`] declares it by hand — and the same absence of a
//! middleman is why [`crate::guard`] exists: an MCP server ran the URL policy, and now this crate
//! does.
//!
//! Layering, outermost first: [`plugin`] mounts [`tools`], which call [`fetch`] and [`search`],
//! which are gated by [`guard`] and rendered by `pai-web-core`.

#[cfg(test)]
pub(crate) mod fake;
pub mod fetch;
pub mod guard;
pub mod plugin;
pub mod search;
pub mod tools;

pub use fetch::{Fetched, Fetcher, Limits};
pub use guard::{Guard, GuardError};
pub use plugin::WebPlugin;
pub use search::{Brave, SearchError, SearchHit, SearchProvider};
pub use tools::{WebFetch, WebSearch};
