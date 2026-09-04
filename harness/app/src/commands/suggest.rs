//! Material for the empty-screen question suggestions, taken from the core because a canned suggestion is
//! either too generic to teach anything or names something absent from this project. It reads whatever the
//! index already has -- no `sync()` in front of the user -- and returns empty rather than erroring.

use pai_index::Index;
use pai_rag::Docs;
use tauri::State;

use crate::AppState;
use crate::protocol::PromptSeeds;

/// Cap per material kind: the UI shows at most five chips and keeps some static ones, so more would be fetched and discarded.
const MAX_SEEDS: usize = 3;

/// Names longer than a chip can show, truncated in the core rather than the UI, by Unicode characters rather
/// than bytes.
const MAX_LEN: usize = 48;

fn short(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_LEN {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_LEN - 1).collect();
    format!("{}…", cut.trim_end())
}

#[tauri::command]
pub async fn prompt_seeds(state: State<'_, AppState>) -> Result<PromptSeeds, String> {
    let harness = state.harness().await?;

    if let Some(index) = harness.ctx.get::<Index>() {
        // A broken index is no reason for the welcome screen to show an error; the static set still works.
        let Ok(stats) = index.stats().await else {
            return Ok(PromptSeeds::default());
        };
        if stats.files == 0 {
            return Ok(PromptSeeds::default());
        }
        let Ok(map) = index.overview().await else {
            return Ok(PromptSeeds::default());
        };
        return Ok(PromptSeeds {
            symbols: map
                .central
                .iter()
                .take(MAX_SEEDS)
                .map(|central| short(&central.node.name))
                .collect(),
            directories: map
                .directories
                .iter()
                .take(MAX_SEEDS)
                .map(|folder| short(&folder.path))
                .collect(),
            documents: Vec::new(),
        });
    }

    if let Some(docs) = harness.ctx.get::<Docs>() {
        let Ok(documents) = docs.documents().await else {
            return Ok(PromptSeeds::default());
        };
        return Ok(PromptSeeds {
            symbols: Vec::new(),
            directories: Vec::new(),
            documents: documents
                .into_iter()
                .take(MAX_SEEDS)
                .map(|doc| short(&doc.title))
                .collect(),
        });
    }

    Ok(PromptSeeds::default())
}

#[cfg(test)]
mod tests {
    use super::short;

    #[test]
    fn giu_nguyen_ten_ngan() {
        assert_eq!(short("CentralSymbol"), "CentralSymbol");
    }

    #[test]
    fn cat_ten_dai_va_them_dau_ba_cham() {
        let long = "a".repeat(80);
        let cut = short(&long);
        assert_eq!(cut.chars().count(), super::MAX_LEN);
        assert!(cut.ends_with('…'));
    }

    /// Truncate by character, not byte: an accented letter spans several bytes and cutting inside one panics.
    #[test]
    fn cat_theo_ky_tu_khong_theo_byte() {
        let long = "đường".repeat(30);
        assert_eq!(short(&long).chars().count(), super::MAX_LEN);
    }

    #[test]
    fn bo_khoang_trang_thua() {
        assert_eq!(short("  Hợp đồng thuê nhà  "), "Hợp đồng thuê nhà");
    }
}
