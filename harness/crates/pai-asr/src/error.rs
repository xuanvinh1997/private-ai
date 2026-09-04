//! Speech-recognition errors.
//! The split mirrors [`pai_rag::RagError`]: what the user can fix (choose a model, pick another
//! file) is separated from what is simply broken, because the document library fingerprints one
//! and retries the other.

/// Errors at the speech-recognition layer.
#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    /// No model chosen, or the microphone is not available. A real capability that is currently off.
    #[error("{0}")]
    Unavailable(String),

    /// This file's bytes cannot become audio: unknown container, no audio track, damaged stream.
    /// Retrying the same bytes cannot produce a different answer.
    #[error("{0}")]
    Decode(String),

    /// The model was loaded and run, and it failed.
    #[error("{0}")]
    Engine(String),
}
