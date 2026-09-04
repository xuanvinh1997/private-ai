//! `@` completion in the composer: every visible file path from the open project, served from the index's
//! cheap path inventory rather than a directory walk on every keystroke. Code projects answer from that
//! inventory, document projects from documents, no project from empty.

use pai_index::Index;
use pai_rag::Docs;
use tauri::State;

use crate::AppState;

/// Hard cap on suggestions: a list longer than the visible area only pushes the useful part off screen.
const MAX_HITS: usize = 20;

#[tauri::command]
pub async fn complete_paths(
    state: State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<String>, String> {
    let harness = state.harness().await?;
    let limit = limit.clamp(1, MAX_HITS);

    if let Some(index) = harness.ctx.get::<Index>() {
        return index
            .paths(&query, limit)
            .await
            .map_err(|err| err.to_string());
    }

    if let Some(docs) = harness.ctx.get::<Docs>() {
        let paths: Vec<String> = docs
            .documents()
            .await
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(|doc| doc.path.display().to_string())
            .collect();
        // The same scorer as code projects: a second one here would rank the same query differently in two screens.
        return Ok(pai_index::complete::rank(&paths, &query, limit));
    }

    Ok(Vec::new())
}
