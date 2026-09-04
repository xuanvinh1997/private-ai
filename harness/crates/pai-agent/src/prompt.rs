//! Assemble the system prompt from ordered sections contributed by plugins.
//! The order is a trust boundary, not a layout preference: identity and operator rules come
//! first, retrieved data last, so no found text speaks before our own rules do.

use std::sync::Arc;

use pai_core::ServiceKey;
use parking_lot::RwLock;

/// Lower numbers sit closer to the top of the prompt.
pub mod order {
    /// Who we are, what we do, what we do not.
    pub const IDENTITY: i32 = 0;
    /// Operator-written packaged procedures. Trusted.
    pub const SKILLS: i32 = 100;
    /// Working directory, project layout, repo conventions.
    pub const WORKSPACE: i32 = 200;
    /// Personal memory.
    pub const MEMORY: i32 = 300;
    /// Document excerpts and web results. Untrusted — always last.
    pub const RETRIEVED: i32 = 900;
}

/// A section is recomputed on every assembly, so it is a function rather than a string.
type Render = Arc<dyn Fn() -> Option<String> + Send + Sync>;

struct Section {
    id: u64,
    order: i32,
    text: Render,
}

/// The register of prompt sections.
#[derive(Default)]
pub struct SystemPrompt {
    sections: RwLock<Vec<Section>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl SystemPrompt {
    pub fn new() -> Arc<SystemPrompt> {
        Arc::new(SystemPrompt::default())
    }

    /// Contribute a section, recomputed on every assembly so a changed working directory changes the prompt.
    pub fn contribute(
        self: &Arc<Self>,
        order: i32,
        text: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> pai_core::Guard {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.sections.write().push(Section {
            id,
            order,
            text: Arc::new(text),
        });
        let registry = self.clone();
        pai_core::Guard::new(move || {
            registry.sections.write().retain(|section| section.id != id);
        })
    }

    pub fn assemble(&self) -> String {
        let mut sections: Vec<(i32, u64, Render)> = self
            .sections
            .read()
            .iter()
            .map(|s| (s.order, s.id, s.text.clone()))
            .collect();
        // Registration order breaks `order` ties, so the prompt is deterministic across runs.
        sections.sort_unstable_by_key(|(order, id, _)| (*order, *id));
        sections
            .into_iter()
            .filter_map(|(_, _, text)| text())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub enum Prompt {}
impl ServiceKey for Prompt {
    type Api = SystemPrompt;
    const NAME: &'static str = "system-prompt";
}
