//! The recognizer itself: one lazily-loaded model, and the two things the app asks of it.
//!
//! A GGUF speech model is hundreds of megabytes and takes seconds to load, so it is loaded on
//! first use and then kept -- but keyed by the path it came from, because the setting can change
//! under us and a stale model would answer in the wrong language forever.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use parking_lot::Mutex;
use transcribe_cpp::{Model, RunOptions, TimestampKind};

use crate::audio::{self, Clip};
use crate::config::AsrConfig;
use crate::error::AsrError;

/// Group segments into blocks of this length, one Markdown heading each. Five minutes is short
/// enough that a citation points somewhere findable in the recording, and long enough that the
/// headings do not outnumber the sentences.
const BLOCK_MS: i64 = 5 * 60 * 1_000;

/// What the settings screen shows about the chosen model, so "why is this off" is answerable
/// without reading a log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub path: PathBuf,
    pub arch: String,
    pub variant: String,
    pub backend: String,
    pub streaming: bool,
    pub languages: Vec<String>,
}

/// One line of a transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A finished transcription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
    pub lines: Vec<Line>,
    pub duration_ms: u64,
}

impl Transcription {
    /// The transcript as the document library wants it: Markdown, with a heading every
    /// [`BLOCK_MS`] so a chunk carries the moment it came from into its citation.
    /// Without timestamps there is nothing to head the blocks with, so the text goes through whole.
    pub fn to_markdown(&self, title: &str) -> String {
        let mut out = format!("# {title}\n\n");
        if let Some(language) = &self.language {
            out.push_str(&format!("*Ngôn ngữ nhận ra: {language}*\n\n"));
        }
        if self.lines.is_empty() {
            out.push_str(self.text.trim());
            out.push('\n');
            return out;
        }
        let mut block = -1_i64;
        for line in &self.lines {
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            let current = line.start_ms / BLOCK_MS;
            if current != block {
                block = current;
                let start = block * BLOCK_MS;
                out.push_str(&format!(
                    "\n## {} – {}\n\n",
                    clock(start),
                    clock(start + BLOCK_MS)
                ));
            }
            out.push_str(text);
            out.push('\n');
        }
        out
    }
}

/// `h:mm:ss`, or `mm:ss` for anything under an hour.
fn clock(ms: i64) -> String {
    let total = (ms.max(0) / 1_000) as u64;
    let (hours, minutes, seconds) = (total / 3_600, (total % 3_600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// The speech recognizer. Cheap to clone; every clone shares the loaded model.
#[derive(Clone)]
pub struct Asr {
    inner: Arc<Inner>,
}

struct Inner {
    config: Mutex<AsrConfig>,
    /// The model and the path it was loaded from, so a changed setting invalidates it.
    loaded: Mutex<Option<(PathBuf, Model)>>,
}

impl std::fmt::Debug for Asr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asr")
            .field("config", &*self.inner.config.lock())
            .field("loaded", &self.inner.loaded.lock().as_ref().map(|(path, _)| path.clone()))
            .finish()
    }
}

impl Asr {
    pub fn new(config: AsrConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                config: Mutex::new(config),
                loaded: Mutex::new(None),
            }),
        }
    }

    pub fn config(&self) -> AsrConfig {
        self.inner.config.lock().clone()
    }

    /// Adopt a new setting. Changing the model path drops the loaded model; the next call pays
    /// the load again, which is the only honest way to answer with the model the user chose.
    pub fn set_config(&self, config: AsrConfig) {
        let mut current = self.inner.config.lock();
        if current.model != config.model {
            *self.inner.loaded.lock() = None;
        }
        *current = config;
    }

    /// Whether a model is configured at all. Not "is it valid" -- that costs a load.
    pub fn configured(&self) -> bool {
        self.inner.config.lock().model_path().is_some()
    }

    /// Load the model if needed and report what it is. Also the settings screen's probe: a model
    /// that loads here is a model the two features can use.
    pub async fn describe(&self) -> Result<ModelInfo, AsrError> {
        let inner = self.inner.clone();
        blocking(move || {
            let (path, model) = load(&inner)?;
            let capabilities = model.capabilities();
            Ok(ModelInfo {
                path,
                arch: model.arch(),
                variant: model.variant(),
                backend: model.backend(),
                streaming: capabilities.supports_streaming,
                languages: capabilities.languages,
            })
        })
        .await
    }

    /// Transcribe one audio file end to end. Decoding and recognition are both blocking CPU/GPU
    /// work and both happen off the async runtime.
    pub async fn transcribe_file(&self, path: &Path) -> Result<Transcription, AsrError> {
        let inner = self.inner.clone();
        let path = path.to_owned();
        blocking(move || {
            let clip = audio::decode_file(&path)?;
            transcribe_clip(&inner, clip)
        })
        .await
    }

    /// Transcribe already-decoded 16 kHz mono audio. This is the path a finished dictation takes
    /// when it is saved as a recording, so the file and the microphone agree on their output.
    pub async fn transcribe_pcm(&self, samples: Vec<f32>) -> Result<Transcription, AsrError> {
        let inner = self.inner.clone();
        blocking(move || {
            transcribe_clip(
                &inner,
                Clip {
                    samples,
                    source_rate: audio::SAMPLE_RATE,
                    channels: 1,
                },
            )
        })
        .await
    }

    pub(crate) fn model(&self) -> Result<Model, AsrError> {
        load(&self.inner).map(|(_, model)| model)
    }

    pub(crate) fn run_options(&self) -> RunOptions {
        RunOptions {
            language: self.inner.config.lock().language_hint(),
            ..RunOptions::default()
        }
    }
}

fn transcribe_clip(inner: &Inner, clip: Clip) -> Result<Transcription, AsrError> {
    let duration_ms = clip.duration_ms();
    let (_, model) = load(inner)?;
    let mut session = model
        .session()
        .map_err(|error| AsrError::Engine(format!("không mở được phiên nhận dạng: {error}")))?;
    let options = RunOptions {
        // Ask for the finest granularity this model actually produces: anything finer is a clean
        // Unsupported, and anything coarser throws away the timestamps a citation needs.
        timestamps: model.capabilities().max_timestamp_kind,
        language: inner.config.lock().language_hint(),
        ..RunOptions::default()
    };
    let result = session
        .run(&clip.samples, &options)
        .map_err(|error| AsrError::Engine(format!("nhận dạng thất bại: {error}")))?;

    let lines = if result.timestamp_kind == TimestampKind::None {
        Vec::new()
    } else {
        result
            .segments
            .iter()
            .map(|segment| Line {
                start_ms: segment.t0_ms,
                end_ms: segment.t1_ms,
                text: segment.text.trim().to_owned(),
            })
            .filter(|line| !line.text.is_empty())
            .collect()
    };
    Ok(Transcription {
        text: result.text.trim().to_owned(),
        language: result.language,
        lines,
        duration_ms,
    })
}

/// Load once, then hand out clones. `Model` is `Arc`-backed, so a clone costs a refcount.
fn load(inner: &Inner) -> Result<(PathBuf, Model), AsrError> {
    let config = inner.config.lock().clone();
    let path = config
        .model_path()
        .ok_or_else(|| {
            AsrError::Unavailable(
                "chưa chọn mô hình nhận dạng tiếng nói; chọn một tệp .gguf trong Cài đặt".into(),
            )
        })?
        .to_owned();

    let mut slot = inner.loaded.lock();
    if let Some((loaded_path, model)) = slot.as_ref()
        && loaded_path == &path
    {
        return Ok((loaded_path.clone(), model.clone()));
    }
    if !path.is_file() {
        return Err(AsrError::Unavailable(format!(
            "không thấy mô hình nhận dạng tại `{}`",
            path.display()
        )));
    }

    // A no-op in the static build we ship, and the documented way to be right in any other.
    static BACKENDS: Once = Once::new();
    BACKENDS.call_once(|| {
        if let Err(error) = transcribe_cpp::init_backends_default() {
            tracing::warn!(%error, "không nạp được backend tính toán cho nhận dạng tiếng nói");
        }
    });

    let model = Model::load(&path).map_err(|error| {
        AsrError::Unavailable(format!(
            "không nạp được mô hình `{}`: {error}",
            path.display()
        ))
    })?;
    tracing::info!(
        path = %path.display(),
        arch = %model.arch(),
        backend = %model.backend(),
        "đã nạp mô hình nhận dạng tiếng nói"
    );
    *slot = Some((path.clone(), model.clone()));
    Ok((path, model))
}

/// Run blocking work on the blocking pool, turning a cancelled task into a readable sentence.
async fn blocking<T, F>(work: F) -> Result<T, AsrError>
where
    F: FnOnce() -> Result<T, AsrError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| AsrError::Engine(format!("tác vụ nhận dạng bị dừng: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_ms: i64, text: &str) -> Line {
        Line {
            start_ms,
            end_ms: start_ms + 2_000,
            text: text.into(),
        }
    }

    #[test]
    fn clock_grows_an_hour_field_only_when_there_is_one() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65_000), "1:05");
        assert_eq!(clock(3_725_000), "1:02:05");
    }

    #[test]
    fn each_five_minute_block_gets_one_heading() {
        let transcription = Transcription {
            text: "a b c".into(),
            language: Some("vi".into()),
            lines: vec![
                line(0, "Câu đầu."),
                line(60_000, "Vẫn khối đầu."),
                line(6 * 60_000, "Khối sau."),
            ],
            duration_ms: 7 * 60_000,
        };
        let markdown = transcription.to_markdown("Họp tuần");
        assert!(markdown.starts_with("# Họp tuần"));
        assert_eq!(markdown.matches("\n## ").count(), 2);
        assert!(markdown.contains("## 0:00 – 5:00"));
        assert!(markdown.contains("## 5:00 – 10:00"));
    }

    /// The whole path, with the real model: decode a container this machine can read, recognize it, and
    /// come back with words. Ignored by default because it loads half a gigabyte of weights -- run it
    /// with a model in `~/.private-ai/asr/models` and a `.wav` beside it.
    ///
    ///     cargo test -p pai-asr -- --ignored
    #[tokio::test]
    #[ignore = "nạp mô hình thật trong ~/.private-ai/asr/models"]
    async fn a_real_model_reads_a_real_recording() {
        let home = std::env::var_os("HOME").map(PathBuf::from).expect("HOME");
        let data = home.join(".private-ai");
        let model = crate::config::discover_model(&data).expect("một tệp .gguf trong asr/models");
        let audio = std::env::var_os("PAI_ASR_TEST_AUDIO")
            .map(PathBuf::from)
            .expect("PAI_ASR_TEST_AUDIO trỏ tới tệp âm thanh");

        let asr = Asr::new(AsrConfig {
            enabled: true,
            model,
            language: String::new(),
        });
        let info = asr.describe().await.expect("nạp được mô hình");
        assert!(!info.arch.is_empty());

        let transcription = asr.transcribe_file(&audio).await.expect("đọc được tệp");
        assert!(
            !transcription.text.trim().is_empty(),
            "phải nghe ra chữ trong {}",
            audio.display()
        );
        assert!(transcription.duration_ms > 0);
    }

    /// No timestamps is not a failure: the text is the whole point, and a transcript without
    /// headings still chunks, embeds and answers questions.
    #[test]
    fn a_model_without_timestamps_still_produces_a_document() {
        let transcription = Transcription {
            text: "Toàn bộ nội dung.".into(),
            language: None,
            lines: Vec::new(),
            duration_ms: 1_000,
        };
        let markdown = transcription.to_markdown("Ghi âm");
        assert!(markdown.contains("Toàn bộ nội dung."));
        assert!(!markdown.contains("##"));
    }
}
