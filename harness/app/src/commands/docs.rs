//! The document library: ingest, list, delete, and test-search. No model tools appear here on purpose --
//! ingesting and deleting are user actions, and the model only gets `docs.search`, `docs.read` and
//! `docs.list`: it reads the library, it does not rearrange it.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::BoxStream;
use pai_rag::{DocLibrary, Docs, Document, IngestEvent};
use tauri::State;
use tauri::ipc::Channel;

use crate::AppState;
use crate::harness::Harness;
use crate::protocol::{DocumentHit, DocumentView, IngestProgress, LibraryStats};

/// The open project's library; absence is valid, since code projects do not load `rag`, and the answer says
/// which project type this is rather than reporting a technical error.
fn library(harness: &Harness) -> Result<Arc<dyn DocLibrary>, String> {
    harness
        .ctx
        .get::<Docs>()
        .ok_or_else(|| "dự án đang mở không phải thư viện tài liệu".to_string())
}

fn view(doc: Document) -> DocumentView {
    DocumentView {
        id: doc.id,
        // The real path inside the project, not where the file came from: `origin` may have moved long ago.
        path: doc.path.display().to_string(),
        title: doc.title,
        format: doc.format.as_str().to_string(),
        bytes: doc.bytes,
        chunks: doc.chunks,
        embedded: doc.embedded,
        added_at: doc.added_at,
        error: doc.error,
    }
}

/// Drain an ingest stream, forwarding each milestone to the UI, then return the whole library; the three
/// commands below differ only in which stream they pass.
async fn drain(
    library: &Arc<dyn DocLibrary>,
    mut stream: BoxStream<'_, IngestEvent>,
    on_progress: Channel<IngestProgress>,
) -> Result<Vec<DocumentView>, String> {
    while let Some(event) = stream.next().await {
        let progress = IngestProgress {
            path: event.path,
            stage: event.stage.as_str().to_string(),
            done: event.done,
            total: event.total,
            finished: event.finished,
            error: event.error,
        };
        if let Err(err) = on_progress.send(progress) {
            // A closed window is no reason to abandon the ingest: files already copied into the store would be left half-done.
            tracing::debug!("could not send ingest progress: {err}");
        }
    }
    drop(stream);

    Ok(library
        .documents()
        .await
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(view)
        .collect())
}

#[tauri::command]
pub async fn list_documents(state: State<'_, AppState>) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    Ok(library(&harness)?
        .documents()
        .await
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(view)
        .collect())
}

#[tauri::command]
pub async fn library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let harness = state.harness().await?;
    let stats = library(&harness)?.stats().await.map_err(|err| err.to_string())?;
    Ok(LibraryStats {
        documents: stats.documents,
        chunks: stats.chunks,
        embedded_chunks: stats.embedded_chunks,
        embedder: stats.embedder,
        semantic_ready: stats.semantic_ready,
        reason: stats.reason,
        root: stats.root.display().to_string(),
        files_seen: stats.files_seen,
        files_skipped: stats.files_skipped,
        unreadable: stats.unreadable,
        excluded: stats.excluded,
        scanned_at: stats.scanned_at,
        scanning: stats.scanning.map(|item| crate::protocol::ScanProgress {
            done: item.done,
            total: item.total,
        }),
    })
}

/// Ingest a batch, emit progress, and return the whole library rather than the additions, since re-ingesting
/// updates existing rows. One bad file never fails the batch -- that is `pai-rag`'s contract, unchanged here.
#[tauri::command]
pub async fn add_documents(
    paths: Vec<String>,
    on_progress: Channel<IngestProgress>,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    let library = library(&harness)?;
    let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();

    let stream = library.ingest(files);
    drain(&library, stream, on_progress).await
}

/// Rescan the project directory and sync the library with it, because the project directory is the library.
/// Not done inside `Library::open`, where a synchronous scan would freeze the window with nothing on screen.
#[tauri::command]
pub async fn sync_library(
    on_progress: Channel<IngestProgress>,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    let library = library(&harness)?;

    let stream = library.sync();
    drain(&library, stream, on_progress).await
}

/// Reprocess the whole library on an explicit click. The automatic scan is incremental, so a file that failed
/// for a since-fixed reason is never revisited; this forgets every fingerprint, re-extracts, and embeds what
/// is missing. It is expensive, hence a button rather than automatic.
#[tauri::command]
pub async fn reprocess_library(
    on_progress: Channel<IngestProgress>,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    let library = library(&harness)?;

    let stream = library.reprocess();
    drain(&library, stream, on_progress).await
}

#[tauri::command]
pub async fn remove_document(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    library(&harness)?
        .remove(&id)
        .await
        .map_err(|err| err.to_string())
}

/// A test search, so the user can verify the library before asking the assistant; matches come back verbatim
/// and the UI must render them as quotations, since this is third-party text.
#[tauri::command]
pub async fn search_documents(
    query: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentHit>, String> {
    let harness = state.harness().await?;
    Ok(library(&harness)?
        .search(&query, limit.unwrap_or(8))
        .await
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|hit| DocumentHit {
            document_id: hit.document_id,
            title: hit.title,
            path: hit.path.display().to_string(),
            ordinal: hit.ordinal,
            text: hit.text,
            score: hit.score,
            matched_by: hit.matched_by.as_str().to_string(),
        })
        .collect())
}
