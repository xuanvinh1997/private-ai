//! The agent loop: the heart of the product, and deliberately the smallest part.
//! It knows four hook points and nothing about approval, sandbox or compaction. History
//! comes from the journal, the last round offers no tools, and cancelling still records.

pub mod bridge;
pub mod compaction;
pub mod driver;
pub mod events;
pub mod plugin;
pub mod prompt;
pub mod skills;
pub mod subagent;

pub use compaction::CompactionPlugin;
pub use driver::{Driver, Silent, TurnSink};
pub use events::{
    AgentRequest, PreStep, PreStepRequest, Replacement, StepDecision, TurnStop, TurnStopping,
};
pub use plugin::AgentPlugin;
pub use prompt::{Prompt, SystemPrompt, order};
pub use skills::{Skill, SkillRegistry, SkillsPlugin};
pub use subagent::{
    LocalSubagents, MAX_DEPTH, SubagentPlugin, SubagentProvider, SubagentReport, Subagents, Task,
};
