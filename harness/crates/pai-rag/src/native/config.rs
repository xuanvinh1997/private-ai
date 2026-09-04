use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::RagError;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderConfig {
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub dim: Option<usize>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "ollama".into(),
            base_url: "http://127.0.0.1:11434".into(),
            api_key: String::new(),
            model: String::new(),
            dim: None,
        }
    }
}

impl ProviderConfig {
    pub fn root(&self) -> String {
        let value = self.base_url.trim().trim_end_matches('/');
        let tail = value.rsplit('/').next().unwrap_or_default();
        if tail
            .strip_prefix('v')
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        {
            value[..value.len() - tail.len()]
                .trim_end_matches('/')
                .to_owned()
        } else {
            value.to_owned()
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct VectorConfig {
    pub url: String,
    pub api_key: String,
    pub collection_prefix: String,
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:6333".into(),
            api_key: String::new(),
            collection_prefix: "pai_docs".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ChunkConfig {
    pub size: usize,
    pub overlap: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            size: 1_400,
            overlap: 180,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RerankConfig {
    pub enabled: bool,
    pub backend: String,
    pub model: String,
    pub candidates: usize,
    pub top_n: usize,
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct OcrConfig {
    pub enabled: bool,
    pub min_chars_per_page: usize,
    pub max_pages: usize,
    pub scale: f32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_chars_per_page: 200,
            max_pages: 120,
            scale: 2.0,
        }
    }
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: "http".into(),
            model: String::new(),
            candidates: 30,
            top_n: 8,
            url: String::new(),
            api_key: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
struct ProjectConfig {
    id: String,
    #[allow(dead_code)]
    name: String,
    root: PathBuf,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            root: PathBuf::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
struct FileConfig {
    version: u32,
    data_dir: PathBuf,
    projects: Vec<ProjectConfig>,
    active_project: String,
    embedding: ProviderConfig,
    vision: ProviderConfig,
    vectors: VectorConfig,
    chunk: ChunkConfig,
    ocr: OcrConfig,
    rerank: RerankConfig,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            version: 1,
            data_dir: PathBuf::new(),
            projects: Vec::new(),
            active_project: String::new(),
            embedding: ProviderConfig::default(),
            vision: ProviderConfig::default(),
            vectors: VectorConfig::default(),
            chunk: ChunkConfig::default(),
            ocr: OcrConfig::default(),
            rerank: RerankConfig::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NativeConfig {
    pub project: String,
    pub root: PathBuf,
    pub store_path: PathBuf,
    pub embedding: ProviderConfig,
    pub vision: ProviderConfig,
    pub vectors: VectorConfig,
    pub chunk: ChunkConfig,
    pub ocr: OcrConfig,
    pub rerank: RerankConfig,
}

impl NativeConfig {
    pub fn load(path: &Path, wanted: &str, fallback_root: &Path) -> Result<Self, RagError> {
        let raw = std::fs::read_to_string(path).map_err(|error| {
            RagError::Service(format!(
                "không đọc được cấu hình RAG `{}`: {error}",
                path.display()
            ))
        })?;
        let parsed: FileConfig = serde_json::from_str(&raw).map_err(|error| {
            RagError::Service(format!(
                "cấu hình RAG `{}` không phải JSON hợp lệ: {error}",
                path.display()
            ))
        })?;
        if parsed.version != 1 {
            return Err(RagError::Service(format!(
                "không hỗ trợ phiên bản cấu hình RAG {}",
                parsed.version
            )));
        }
        let project_id = if wanted.trim().is_empty() {
            parsed.active_project.trim()
        } else {
            wanted.trim()
        };
        let selected = parsed.projects.iter().find(|item| item.id == project_id);
        let root = selected
            .map(|item| item.root.clone())
            .unwrap_or_else(|| fallback_root.to_owned());
        if project_id.is_empty() {
            return Err(RagError::Unavailable(
                "chưa có dự án tài liệu đang mở".into(),
            ));
        }
        let data_dir = if parsed.data_dir.as_os_str().is_empty() {
            default_data_dir()
        } else {
            parsed.data_dir
        };
        Ok(Self {
            project: project_id.to_owned(),
            root,
            store_path: data_dir.join(project_id).join("rag.sqlite"),
            embedding: parsed.embedding,
            vision: parsed.vision,
            vectors: parsed.vectors,
            chunk: parsed.chunk,
            ocr: parsed.ocr,
            rerank: parsed.rerank,
        })
    }

    pub fn collection(&self) -> String {
        format!("{}_{}", self.vectors.collection_prefix, self.project)
    }

    pub fn purge_parts(
        path: &Path,
        project: &str,
    ) -> Result<(PathBuf, VectorConfig, String), RagError> {
        if project.is_empty()
            || project == "."
            || project == ".."
            || project.contains('/')
            || project.contains('\\')
        {
            return Err(RagError::Service("mã dự án cần xoá không hợp lệ".into()));
        }
        let raw = std::fs::read_to_string(path).map_err(|error| {
            RagError::Service(format!(
                "không đọc được cấu hình RAG `{}`: {error}",
                path.display()
            ))
        })?;
        let parsed: FileConfig = serde_json::from_str(&raw)
            .map_err(|error| RagError::Service(format!("cấu hình RAG không hợp lệ: {error}")))?;
        if parsed.version != 1 {
            return Err(RagError::Service(format!(
                "không hỗ trợ phiên bản cấu hình RAG {}",
                parsed.version
            )));
        }
        let data_dir = if parsed.data_dir.as_os_str().is_empty() {
            default_data_dir()
        } else {
            parsed.data_dir
        };
        let collection = format!("{}_{}", parsed.vectors.collection_prefix, project);
        Ok((data_dir.join(project), parsed.vectors, collection))
    }
}

fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Local"));
        return base.join("private-ai").join("rag");
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("private-ai").join("rag");
    }
    home()
        .join(".local")
        .join("share")
        .join("private-ai")
        .join("rag")
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_version_suffix_from_provider_url() {
        let provider = ProviderConfig {
            base_url: "http://localhost:1234/v1/".into(),
            ..ProviderConfig::default()
        };
        assert_eq!(provider.root(), "http://localhost:1234");
    }
}
