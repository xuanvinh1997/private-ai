//! Path scoring for the `@` completion box in the UI.
//! Not the `search` scorer: FTS5 ranks by BM25, which buries `mod.rs` just as the user
//! types `mod`. Here filename beats directory, prefix beats mid-word, shallower wins.

/// Score one path against already-lowercased tokens, or `None` if any token is missing.
fn score(path: &str, tokens: &[&str]) -> Option<i32> {
    let lower = path.to_ascii_lowercase();
    // Split by hand rather than `Path::file_name`: index paths always use `/`.
    let name = lower.rsplit('/').next().unwrap_or(&lower);

    let mut total = 0;
    for token in tokens {
        let in_name = name.find(token);
        let in_path = lower.find(token);
        total += match (in_name, in_path) {
            // Matches the filename, right at its start.
            (Some(0), _) => 6,
            // Matches the filename, but mid-word.
            (Some(_), _) => 4,
            // Matches only the directory part.
            (None, Some(_)) => 1,
            (None, None) => return None,
        };
    }

    // Break ties by depth, not length; capped at 8 so a very deep file never sinks below a name-mismatch.
    Some(total - (lower.matches('/').count().min(8) as i32))
}

/// Filter and rank paths for a completion query; an empty query returns the shallowest paths, not the alphabetical first.
pub fn rank(paths: &[String], query: &str, limit: usize) -> Vec<String> {
    let lowered = query.to_ascii_lowercase();
    let tokens: Vec<&str> = lowered.split_whitespace().collect();

    if tokens.is_empty() {
        let mut shallow: Vec<&String> = paths.iter().collect();
        shallow.sort_by_key(|path| (path.matches('/').count(), path.len(), (*path).clone()));
        return shallow.into_iter().take(limit).cloned().collect();
    }

    let mut scored: Vec<(i32, &String)> = paths
        .iter()
        .filter_map(|path| score(path, &tokens).map(|points| (points, path)))
        .collect();

    // Score, then length, then alphabetical: a list that reorders between keystrokes gets misclicked.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });
    scored.into_iter().take(limit).map(|(_, p)| p.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<String> {
        [
            "crates/pai-index/src/store.rs",
            "crates/pai-index/src/complete.rs",
            "crates/pai-fs/src/tools/read.rs",
            "crates/pai-fs/src/lib.rs",
            "crates/pai-core/src/lib.rs",
            "README.md",
            "docs/ARCHITECTURE.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn ten_tep_thang_ten_thu_muc() {
        // `store` matches the name `store.rs`; no other file has `store` in its name.
        let hits = rank(&paths(), "store", 5);
        assert_eq!(hits[0], "crates/pai-index/src/store.rs");
    }

    #[test]
    fn moi_token_phai_khop() {
        let hits = rank(&paths(), "pai fs read", 5);
        assert_eq!(hits, vec!["crates/pai-fs/src/tools/read.rs"]);
    }

    #[test]
    fn khong_khop_thi_rong() {
        assert!(rank(&paths(), "khongcogi", 5).is_empty());
    }

    #[test]
    fn cung_ten_thi_nong_hon_truoc() {
        // Both `lib.rs` score the same and sit at the same depth, but must beat directory-only matches.
        let hits = rank(&paths(), "lib", 5);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|p| p.ends_with("lib.rs")));
    }

    #[test]
    fn truy_van_rong_tra_ve_tep_nong_nhat() {
        let hits = rank(&paths(), "", 2);
        assert_eq!(hits[0], "README.md");
    }

    #[test]
    fn khop_dau_ten_tren_khop_giua_ten() {
        let list = vec!["src/complete.rs".to_string(), "src/precompute.rs".to_string()];
        assert_eq!(rank(&list, "comp", 2)[0], "src/complete.rs");
    }

    #[test]
    fn ton_trong_limit() {
        assert_eq!(rank(&paths(), "rs", 2).len(), 2);
    }
}
