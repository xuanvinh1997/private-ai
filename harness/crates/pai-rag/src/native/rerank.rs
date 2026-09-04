//! Local ONNX cross-encoder reranking.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fastembed::{
    OnnxSource, RerankInitOptionsUserDefined, TextRerank, TokenizerFiles, UserDefinedRerankingModel,
};

use super::config::RerankConfig;
use crate::RagError;

pub const MODEL_ID: &str = "BAAI/bge-reranker-v2-m3";
const MODEL_DIRECTORY: &str = "bge-reranker-v2-m3";
const MODEL_FILE: &str = "model_quantized.onnx";
const MAX_LENGTH: usize = 1_024;
const BATCH_SIZE: usize = 4;

#[derive(Clone, Debug)]
pub struct LocalReranker {
    configured_dir: PathBuf,
    model: Arc<Mutex<Option<TextRerank>>>,
}

#[derive(Clone, Debug)]
pub struct Scored {
    pub index: usize,
    pub score: f32,
}

impl LocalReranker {
    pub fn new(config: &RerankConfig) -> Self {
        Self {
            configured_dir: config.path.clone(),
            model: Arc::new(Mutex::new(None)),
        }
    }

    /// Load once, then serialize calls through ONNX Runtime's mutable session. Loading and inference are
    /// blocking CPU work, so neither is allowed to occupy a Tokio worker thread.
    pub async fn score(
        &self,
        query: &str,
        passages: &[&str],
        limit: usize,
    ) -> Result<Vec<Scored>, RagError> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }
        let configured_dir = self.configured_dir.clone();
        let model = self.model.clone();
        let query = query.to_owned();
        let passages: Vec<String> = passages.iter().map(|text| (*text).to_owned()).collect();
        tokio::task::spawn_blocking(move || {
            let mut slot = model
                .lock()
                .map_err(|_| RagError::Service("khóa ONNX reranker bị poison".into()))?;
            if slot.is_none() {
                *slot = Some(load(&configured_dir)?);
            }
            let ranked = slot
                .as_mut()
                .expect("reranker was initialized above")
                .rerank(query, &passages, false, Some(BATCH_SIZE))
                .map_err(|error| RagError::Service(format!("ONNX rerank thất bại: {error}")))?;
            Ok(ranked
                .into_iter()
                .take(limit.min(passages.len()))
                .map(|item| Scored {
                    index: item.index,
                    // BGE returns one relevance logit. Sigmoid makes the score useful to callers while
                    // preserving the exact ordering.
                    score: 1.0 / (1.0 + (-item.score).exp()),
                })
                .collect())
        })
        .await
        .map_err(|error| RagError::Service(format!("tác vụ ONNX rerank bị dừng: {error}")))?
    }
}

fn load(configured_dir: &Path) -> Result<TextRerank, RagError> {
    let directory = model_directory(configured_dir)?;
    let read = |name: &str| {
        std::fs::read(directory.join(name)).map_err(|error| {
            RagError::Service(format!(
                "không đọc được model rerank `{}`: {error}",
                directory.join(name).display()
            ))
        })
    };
    let model = UserDefinedRerankingModel::new(
        OnnxSource::File(directory.join(MODEL_FILE)),
        TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        },
    );
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(1, 4);
    TextRerank::try_new_from_user_defined(
        model,
        RerankInitOptionsUserDefined::new()
            .with_max_length(MAX_LENGTH)
            .with_intra_threads(threads),
    )
    .map_err(|error| RagError::Service(format!("không nạp được {MODEL_ID} ONNX: {error}")))
}

fn model_directory(configured: &Path) -> Result<PathBuf, RagError> {
    let mut candidates = Vec::new();
    if !configured.as_os_str().is_empty() {
        candidates.push(configured.to_owned());
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        // Windows/Linux resources live beside the executable. A macOS app keeps them under
        // Contents/Resources while its executable is under Contents/MacOS.
        candidates.push(parent.join("models").join(MODEL_DIRECTORY));
        candidates.push(
            parent
                .join("resources")
                .join("models")
                .join(MODEL_DIRECTORY),
        );
        if let Some(contents) = parent.parent() {
            candidates.push(
                contents
                    .join("Resources")
                    .join("models")
                    .join(MODEL_DIRECTORY),
            );
        }
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../app/models")
            .join(MODEL_DIRECTORY),
    );

    if let Some(directory) = candidates.iter().find(|directory| valid_model(directory)) {
        return Ok(directory.clone());
    }
    Err(RagError::Unavailable(format!(
        "chưa có {MODEL_ID} ONNX. Chạy `node scripts/prepare-reranker.mjs`; đã tìm: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn valid_model(directory: &Path) -> bool {
    [
        MODEL_FILE,
        "tokenizer.json",
        "config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    ]
    .iter()
    .all(|name| directory.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_model_directory_must_contain_the_whole_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(MODEL_FILE), b"placeholder").unwrap();
        assert!(!valid_model(directory.path()));
    }

    #[tokio::test]
    #[ignore = "loads the 571 MB ONNX model prepared by scripts/prepare-reranker.mjs"]
    async fn bundled_model_scores_relevant_text_first() {
        let reranker = LocalReranker::new(&RerankConfig::default());
        let ranked = reranker
            .score(
                "Thủ đô của Việt Nam là gì?",
                &["Hà Nội là thủ đô của Việt Nam.", "Cá voi sống dưới biển."],
                2,
            )
            .await
            .unwrap();
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].index, 0);
        assert!(ranked[0].score > ranked[1].score);
    }
}
