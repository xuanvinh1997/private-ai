//! Speech recognition for the desktop app, on the user's own machine.
//!
//! Two features, one model: audio files a document project can read, and a microphone the composer
//! can listen to. Both go through [`transcribe.cpp`](https://github.com/handy-computer/transcribe.cpp)
//! and neither sends a byte anywhere.
//!
//! The library accepts exactly one input shape -- mono `f32` PCM at 16 kHz -- so [`audio`] is not a
//! helper but the front half of both features.

pub mod audio;
pub mod config;
pub mod dictate;
pub mod engine;
pub mod error;

pub use audio::{Clip, MAX_DURATION_MS, SAMPLE_RATE, decode_file, is_audio};
pub use config::{AsrConfig, MODEL_DIR, discover_model};
pub use dictate::{Dictation, DictationControl, DictationEvent, start as dictate};
pub use engine::{Asr, Line, ModelInfo, Transcription};
pub use error::AsrError;
