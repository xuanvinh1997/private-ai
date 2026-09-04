//! Why a chunk showed up in the results.
//! Fusion and cosine now live in `services/rag/.../retrieval/fusion.py`; only this label
//! stays, because [`crate::library::Hit`] carries it and the UI draws it.

use serde::{Deserialize, Serialize};

/// Serde strings match `DocumentHit.matched_by` in `app/` and `matchedBy` on the Python side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Keyword,
    Semantic,
    Both,
}

impl MatchedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchedBy::Keyword => "keyword",
            MatchedBy::Semantic => "semantic",
            MatchedBy::Both => "both",
        }
    }

    /// From the wire string; unknown labels fall back to [`MatchedBy::Keyword`], the branch that needs no embedder.
    pub fn parse(value: &str) -> MatchedBy {
        match value.trim().to_ascii_lowercase().as_str() {
            "semantic" => MatchedBy::Semantic,
            "both" => MatchedBy::Both,
            _ => MatchedBy::Keyword,
        }
    }
}
