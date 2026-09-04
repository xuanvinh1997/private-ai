//! Skills: packaged procedures, one directory with a `SKILL.md` each.
//! Progressive disclosure in three tiers — name plus one line always, full text only when
//! selected, sibling files listed only. Skill text is operator-written, hence trusted.

mod loader;
mod registry;

pub use loader::{Skill, SkillError, load_skill};
pub use registry::{SkillRegistry, SkillsPlugin};
