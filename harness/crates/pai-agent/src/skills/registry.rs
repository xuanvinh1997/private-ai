//! Sổ skill, và việc chọn skill cho một lượt.

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

/// Điểm tối thiểu để một skill được coi là liên quan.
const THRESHOLD: f32 = 2.0;
/// Và nó còn phải đạt ít nhất chừng này so với skill điểm cao nhất. Không có luật thứ
/// hai thì một câu hỏi dài kéo theo mọi skill có chung một từ thông dụng.
const RELATIVE_FLOOR: f32 = 0.5;

#[derive(Default)]
pub struct SkillRegistry {
    skills: RwLock<Vec<Skill>>,
    /// Skill được chọn cho lượt đang chạy. Đặt bởi middleware, đọc bởi khối prompt.
    active: RwLock<Vec<String>>,
}

/// Bỏ dấu tiếng Việt và hạ chữ thường.
///
/// Người dùng gõ "tai lieu" cũng phải chọn được skill khai báo "tài liệu". Không gấp dấu
/// thì cơ chế chọn chỉ chạy đúng khi người ta gõ đủ dấu, tức là gần như không bao giờ.
/// Bảng dấu tiếng Việt, viết ra thành từng nhóm thay vì dùng khoảng mã.
///
/// Các ký tự tiếng Việt nằm rải rác qua nhiều khối Unicode, nên khoảng mã vừa bỏ sót vừa
/// chồng lên nhau. Một bảng dài nhưng đúng thì đọc được và sửa được; một khoảng mã ngắn
/// nhưng sai thì im lặng bỏ qua đúng những chữ hay gặp nhất.
const FOLD: [(&str, char); 7] = [
    ("àáâãäåạảấầẩẫậắằẳẵặăâ", 'a'),
    ("èéêëẹẻẽếềểễệ", 'e'),
    ("ìíîïịỉĩ", 'i'),
    ("òóôõöọỏốồổỗộớờởỡợơô", 'o'),
    ("ùúûüụủũứừửữựư", 'u'),
    ("ỳýỵỷỹ", 'y'),
    ("đ", 'd'),
];

/// Bỏ dấu tiếng Việt và hạ chữ thường.
///
/// Người dùng gõ "tai lieu" cũng phải chọn được skill khai báo "tài liệu". Không gấp dấu
/// thì cơ chế chọn chỉ chạy đúng khi người ta gõ đủ dấu, tức là gần như không bao giờ.
///
/// Hạ chữ thường **trước** rồi mới tra bảng, nên bảng chỉ cần một nửa và không có chữ hoa
/// nào lọt qua vì người viết bảng quên nó.
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

    /// Quét một thư mục. Mỗi thư mục con có `SKILL.md` là một skill.
    ///
    /// Một gói hỏng bị bỏ qua kèm log, không làm hỏng lần quét: mất một skill là mất một
    /// quy trình, còn ném lỗi ở đây là mất cả bộ.
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
                Err(err) => tracing::warn!("bỏ qua skill: {err}"),
            }
        }
        let mut skills = self.skills.write();
        for skill in loaded {
            // Gói của người dùng trùng tên thì **thay thế** gói dựng sẵn, không đứng cạnh.
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

    /// Tầng một: tên và một dòng mô tả của mọi skill. Luôn có mặt trong prompt.
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

    /// Tầng hai: toàn văn hướng dẫn của những skill đã được chọn cho lượt này.
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
                // Tầng ba: chỉ tên tệp. Mô hình tự mở bằng `read` khi thật sự cần.
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

    /// Chọn skill cho một đoạn văn bản. Trùng lặp từ khoá, không gọi mô hình.
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

/// Chọn skill ngay trước khi bước bắt đầu.
///
/// Là middleware trên `agent/pre-step` chứ không phải một lời gọi trong vòng lặp, vì
/// vòng lặp không được biết skill là gì. Gỡ plugin này ra thì prompt mất hai khối và
/// không có gì khác đổi.
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
            // Bước sau trong cùng một lượt không mang message mới; giữ nguyên lựa chọn cũ
            // thay vì xoá, nếu không thì quy trình biến mất giữa chừng đúng lúc mô hình
            // đang theo nó.
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
