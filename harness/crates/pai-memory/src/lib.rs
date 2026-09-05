//! Long-term memory as a knowledge graph on SQLite.
//!
//! Replaces the `@modelcontextprotocol/server-memory` MCP server: same three concepts — entity,
//! observation, relation — so a user's habits carry over, but stored in a queryable database
//! instead of a JSONL file that has to be parsed whole on every call, and running in-process
//! instead of behind an `npx` the machine may not have.
//!
//! [`graph`] owns the data and knows nothing about tools; [`tools`] owns everything the model
//! sees. [`plugin`] is the only place the two meet.

pub mod graph;
pub mod plugin;
pub mod tools;

pub use graph::{
    Entity, EntityInput, Forgotten, Graph, GraphError, GraphResult, Hit, MatchedBy,
    ObservationTarget, Relation, RelationInput, SCHEMA_VERSION, Stats, Written,
};
pub use plugin::{Memory, MemoryPlugin};
pub use tools::SharedGraph;
pub use tools::forget::MemoryForget;
pub use tools::read::MemoryRead;
pub use tools::remember::MemoryRemember;
pub use tools::search::MemorySearch;
