//! Đọc một `SKILL.md`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("{0}: không đọc được: {1}")]
    Unreadable(PathBuf, String),
    #[error("{0}: thiếu khối frontmatter mở đầu bằng `---`")]
    NoFrontmatter(PathBuf),
    #[error("{0}: frontmatter hỏng: {1}")]
    BadFrontmatter(PathBuf, String),
    #[error("{0}: thiếu `{1}`")]
    Missing(PathBuf, &'static str),
}

#[derive(Debug, Deserialize)]
struct Front {
    name: String,
    title: Option<String>,
    description: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub title: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub body: String,
    pub dir: PathBuf,
    /// Tên các tệp khác trong thư mục. Chỉ **tên** — tầng ba của tiết lộ dần.
    pub resources: Vec<String>,
}

/// Chữ thường, số, `.`, `-`, `_`. Tên đi vào prompt và vào tên thư mục, nên nó phải hẹp.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

pub fn load_skill(dir: &Path) -> Result<Skill, SkillError> {
    let path = dir.join("SKILL.md");
    let raw = std::fs::read_to_string(&path)
        .map_err(|err| SkillError::Unreadable(path.clone(), err.to_string()))?;

    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
        .ok_or_else(|| SkillError::NoFrontmatter(path.clone()))?;
    let (front, body) = rest
        .split_once("\n---")
        .ok_or_else(|| SkillError::NoFrontmatter(path.clone()))?;

    let front: Front = serde_norway::from_str(front)
        .map_err(|err| SkillError::BadFrontmatter(path.clone(), err.to_string()))?;

    if !valid_name(&front.name) {
        return Err(SkillError::Missing(path.clone(), "name hợp lệ"));
    }
    // Thiếu mô tả thì gói bị bỏ qua chứ không làm hỏng ứng dụng: mô tả là thứ duy nhất
    // mô hình đọc ở tầng một, nên một skill không có nó thì không bao giờ được chọn.
    let description = front
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .ok_or(SkillError::Missing(path.clone(), "description"))?;

    let body = body.trim_start_matches('-').trim().to_string();
    if body.is_empty() {
        return Err(SkillError::Missing(path, "phần thân hướng dẫn"));
    }

    let mut resources: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name != "SKILL.md" && !name.starts_with('.')).then_some(name)
        })
        .collect();
    resources.sort_unstable();

    Ok(Skill {
        title: front.title.unwrap_or_else(|| front.name.clone()),
        name: front.name,
        description,
        keywords: front.keywords,
        body,
        dir: dir.to_path_buf(),
        resources,
    })
}
