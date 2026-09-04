//! What the ASR layer reads out of `rag-config.json`.

use std::path::PathBuf;

use serde::Deserialize;

/// The `asr` entry of the RAG configuration file.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AsrConfig {
    /// Transcribe audio files found in a document project. Off leaves them out of the library
    /// entirely rather than filing them as broken documents -- the same rule images follow with OCR off.
    pub enabled: bool,
    /// A GGUF model file. Empty means no model chosen, which is not an error: it is the state of a
    /// fresh install, and the message says what to do about it.
    pub model: PathBuf,
    /// Source-language hint as a short code, or empty to let the model detect it. A wrong hint is
    /// worse than none, so the default is empty.
    pub language: String,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: PathBuf::new(),
            language: String::new(),
        }
    }
}

impl AsrConfig {
    /// The chosen model, or `None` when the setting is untouched.
    pub fn model_path(&self) -> Option<&std::path::Path> {
        if self.model.as_os_str().is_empty() {
            None
        } else {
            Some(self.model.as_path())
        }
    }

    pub fn language_hint(&self) -> Option<String> {
        let trimmed = self.language.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }
}

/// The folder a bundled or hand-downloaded model is expected in, under the app's data directory.
pub const MODEL_DIR: &str = "asr/models";

/// Look for a model to start with. A fresh install has no `asr` entry, and asking the user to find a
/// `.gguf` before dictation does anything is a bad first minute -- so if one is already sitting in the
/// data directory, that is the answer. Alphabetical, not "first read": directory order is arbitrary,
/// and a seed that changes between launches is worse than one that is merely a guess.
pub fn discover_model(data_dir: &std::path::Path) -> Option<PathBuf> {
    let directory = data_dir.join("asr").join("models");
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seed_is_the_first_gguf_in_alphabetical_order() {
        let temp = std::env::temp_dir().join(format!("pai-asr-seed-{}", std::process::id()));
        let models = temp.join("asr").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("readme.txt"), b"not a model").unwrap();
        std::fs::write(models.join("z-whisper.gguf"), b"").unwrap();
        std::fs::write(models.join("a-parakeet.gguf"), b"").unwrap();

        assert_eq!(discover_model(&temp), Some(models.join("a-parakeet.gguf")));
        std::fs::remove_dir_all(&temp).unwrap();
    }

    #[test]
    fn an_empty_data_directory_seeds_nothing() {
        assert_eq!(discover_model(std::path::Path::new("/khong-ton-tai")), None);
    }
}
