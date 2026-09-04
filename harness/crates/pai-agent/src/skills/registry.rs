//! The skill register, and choosing skills for a turn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use pai_core::{Context, Middleware, Next, Plugin};
use parking_lot::RwLock;

use crate::events::{PreStep, PreStepRequest, StepDecision};
use crate::prompt::{Prompt, order};
use crate::skills::loader::{Skill, load_skill};

/// The minimum score for a skill to count as relevant.
const THRESHOLD: f32 = 2.0;
/// It must also reach this fraction of the top score, or a long question drags in every skill sharing a common word.
const RELATIVE_FLOOR: f32 = 0.5;

#[derive(Default)]
pub struct SkillRegistry {
    skills: RwLock<Vec<Skill>>,
    /// Skills chosen for the current turn; set by middleware, read by the prompt section.
    active: RwLock<Vec<String>>,
}

/// The Vietnamese fold table, written out as groups: the letters are scattered across Unicode blocks, so ranges miss some.
const FOLD: [(&str, char); 7] = [
    ("àáâãäåạảấầẩẫậắằẳẵặăâ", 'a'),
    ("èéêëẹẻẽếềểễệ", 'e'),
    ("ìíîïịỉĩ", 'i'),
    ("òóôõöọỏốồổỗộớờởỡợơô", 'o'),
    ("ùúûüụủũứừửữựư", 'u'),
    ("ỳýỵỷỹ", 'y'),
    ("đ", 'd'),
];

/// Strip Vietnamese diacritics and lowercase; lowercasing first halves the table and lets no capital slip through.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            FOLD.iter()
                .find_map(|(group, folded)| group.contains(c).then_some(*folded))
                .unwrap_or(c)
        })
        .collect()
}

fn mentions(haystack: &str, needle: &str) -> bool {
    !needle.is_empty() && haystack.contains(needle)
}

impl SkillRegistry {
    pub fn new() -> Arc<SkillRegistry> {
        Arc::new(SkillRegistry::default())
    }

    /// Scan a directory; each subdirectory with a `SKILL.md` is a skill, and a broken one is logged and skipped.
    pub fn scan(&self, root: &Path) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        let mut loaded = Vec::new();
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.join("SKILL.md").is_file() {
                continue;
            }
            match load_skill(&dir) {
                Ok(skill) => loaded.push(skill),
                Err(err) => tracing::warn!("skipping skill: {err}"),
            }
        }
        let mut skills = self.skills.write();
        for skill in loaded {
            // A user package of the same name replaces the built-in one rather than sitting beside it.
            skills.retain(|existing| existing.name != skill.name);
            skills.push(skill);
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn add(&self, skill: Skill) {
        let mut skills = self.skills.write();
        skills.retain(|existing| existing.name != skill.name);
        skills.push(skill);
        skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn len(&self) -> usize {
        self.skills.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Tier one: every skill's name and one-line description, always in the prompt.
    pub fn catalog(&self) -> Option<String> {
        let skills = self.skills.read();
        if skills.is_empty() {
            return None;
        }
        let list = skills
            .iter()
            .map(|skill| format!("- `{}` — {}", skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!(
            "## Quy trình có sẵn\n\nNhững quy trình dưới đây đã được viết sẵn cho các \
             việc thường gặp. Khi một việc khớp với một quy trình, hướng dẫn đầy đủ của \
             nó sẽ được đưa vào lượt đó.\n\n{list}"
        ))
    }

    /// Tier two: the full instructions of the skills selected for this turn.
    pub fn activated(&self) -> Option<String> {
        let active = self.active.read();
        if active.is_empty() {
            return None;
        }
        let skills = self.skills.read();
        let bodies: Vec<String> = active
            .iter()
            .filter_map(|name| skills.iter().find(|skill| &skill.name == name))
            .map(|skill| {
                // Tier three: filenames only; the model opens them with `read` when it needs to.
                let files = if skill.resources.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\nTệp đi kèm trong `{}`: {}",
                        skill.dir.display(),
                        skill.resources.join(", ")
                    )
                };
                format!("### {}\n\n{}{}", skill.title, skill.body, files)
            })
            .collect();
        if bodies.is_empty() {
            return None;
        }
        Some(format!(
            "## Quy trình cho lượt này\n\n{}",
            bodies.join("\n\n")
        ))
    }

    /// Select skills for a piece of text by keyword overlap, with no model call.
    pub fn select(&self, text: &str) -> Vec<String> {
        let folded = fold(text);
        let skills = self.skills.read();
        let mut scored: Vec<(f32, String)> = skills
            .iter()
            .filter_map(|skill| {
                let mut score = 0.0;
                if mentions(&folded, &fold(&skill.name)) {
                    score += 3.0;
                }
                if mentions(&folded, &fold(&skill.title)) {
                    score += 2.0;
                }
                for keyword in &skill.keywords {
                    if mentions(&folded, &fold(keyword)) {
                        score += 2.0;
                    }
                }
                for word in fold(&skill.description)
                    .split_whitespace()
                    .filter(|w| w.len() > 4)
                {
                    if mentions(&folded, word) {
                        score += 0.5;
                    }
                }
                (score >= THRESHOLD).then(|| (score, skill.name.clone()))
            })
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let best = scored.first().map(|(score, _)| *score).unwrap_or(0.0);
        scored
            .into_iter()
            .filter(|(score, _)| *score >= best * RELATIVE_FLOOR)
            .map(|(_, name)| name)
            .collect()
    }

    fn set_active(&self, names: Vec<String>) {
        *self.active.write() = names;
    }
}

/// Select skills just before a step; middleware on `agent/pre-step`, because the loop must not know what a skill is.
struct ActivateSkills {
    registry: Arc<SkillRegistry>,
}

impl Middleware<PreStep> for ActivateSkills {
    fn call<'a>(
        &'a self,
        req: &'a mut PreStepRequest,
        next: Next<'a, PreStep>,
    ) -> BoxFuture<'a, StepDecision> {
        async move {
            let text = req
                .messages
                .iter()
                .flat_map(|message| message.content.iter())
                .filter_map(|block| match block {
                    pai_session::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            // Later steps in a turn carry no new message; keep the previous selection so the procedure does not vanish mid-use.
            if !text.trim().is_empty() {
                self.registry.set_active(self.registry.select(&text));
            }
            next.run(req).await
        }
        .boxed()
    }
}

pub struct SkillsPlugin {
    roots: Vec<PathBuf>,
}

impl SkillsPlugin {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> SkillsPlugin {
        SkillsPlugin {
            roots: roots.into_iter().collect(),
        }
    }
}

#[async_trait]
impl Plugin for SkillsPlugin {
    fn name(&self) -> &'static str {
        "skills"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let registry = SkillRegistry::new();
        for root in &self.roots {
            registry.scan(root);
        }

        let prompt = ctx.require::<Prompt>()?;
        let catalog = registry.clone();
        ctx.keep(prompt.contribute(order::SKILLS, move || catalog.catalog()));
        let activated = registry.clone();
        ctx.keep(prompt.contribute(order::SKILLS + 1, move || activated.activated()));
        ctx.keep(ctx.on_waterfall(Arc::new(ActivateSkills { registry })));
        Ok(())
    }
}
