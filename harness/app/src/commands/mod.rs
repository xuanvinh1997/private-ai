//! Commands the UI calls, split by screen rather than by core crate, so one broken screen means one file.
//! Every command returns `Result<_, String>` where the error is a sentence the user reads directly,
//! never the `Debug` of an error type.

pub mod asr;
pub mod attach;
pub mod chunk;
pub mod complete;
pub mod docs;
pub mod mcp;
pub mod projects;
pub mod providers;
pub mod rerank;
pub mod system;
