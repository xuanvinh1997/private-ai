use std::{cmp::Ordering, collections::HashMap};

use serde::{Deserialize, Serialize};

/// Reciprocal-rank constant used by the document retriever.
pub const RRF_K: f64 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Keyword,
    Semantic,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ranked {
    pub chunk_id: i64,
    pub score: f64,
    pub matched_by: MatchedBy,
}

/// Merge two best-first lists with deterministic reciprocal-rank fusion.
pub fn fuse(keyword: &[i64], semantic: &[i64], limit: usize) -> Vec<Ranked> {
    let mut merged = Vec::new();
    let mut seen = HashMap::new();

    contribute(&mut merged, &mut seen, keyword, MatchedBy::Keyword);
    contribute(&mut merged, &mut seen, semantic, MatchedBy::Semantic);

    merged.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    merged.truncate(limit);
    merged
}

fn contribute(
    merged: &mut Vec<Ranked>,
    seen: &mut HashMap<i64, usize>,
    ids: &[i64],
    source: MatchedBy,
) {
    for (index, chunk_id) in ids.iter().copied().enumerate() {
        let contribution = 1.0 / (RRF_K + index as f64 + 1.0);
        if let Some(&at) = seen.get(&chunk_id) {
            let row = &mut merged[at];
            row.score += contribution;
            if row.matched_by != source {
                row.matched_by = MatchedBy::Both;
            }
        } else {
            seen.insert(chunk_id, merged.len());
            merged.push(Ranked {
                chunk_id,
                score: contribution,
                matched_by: source,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_hits_win_and_ties_are_stable() {
        let ranked = fuse(&[9, 2, 7], &[7, 2, 8], 4);
        assert_eq!(ranked[0].chunk_id, 7);
        assert_eq!(ranked[0].matched_by, MatchedBy::Both);
        assert_eq!(ranked[1].chunk_id, 2);
        assert_eq!(ranked[1].matched_by, MatchedBy::Both);
        assert_eq!(ranked[2].chunk_id, 9);
        assert_eq!(ranked[3].chunk_id, 8);
    }
}
