//! Two tools, one rule for both of them: everything they return was written by strangers.
//!
//! Both declare `returns_untrusted_content`, so the registry appends its warning to the
//! descriptions, and both declare `leaves_device`, which is the flag that has no other source
//! here. `pai-mcp` infers it from a connection's `Reach`, because every MCP tool runs behind a
//! transport that already knows where it goes. A native tool has no transport to ask, so an
//! author who forgets the flag leaves the whole policy layer blind about the first thing in this
//! product that dials out on its own.

pub mod fetch;
pub mod search;

pub use fetch::WebFetch;
pub use search::WebSearch;
