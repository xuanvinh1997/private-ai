//! Listing projects and switching between them.

use pai_project::{CloneEvent, CloneRequest};
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::protocol::{CloneProgress, ProjectKind, ProjectView};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    let current = harness.current_project().map(|open| open.id);
    Ok(harness
        .projects()?
        .into_iter()
        .map(|project| ProjectView::new(project, current.as_deref()))
        .collect())
}

/// Switch projects. Heavy on the core side -- it tears down and rebuilds a plugin branch -- so running turns
/// are cancelled first, since a turn whose tools vanish under it fails inexplicably.
#[tauri::command]
pub async fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectView, String> {
    let harness = state.harness().await?;
    for (_, token) in state.running.lock().drain() {
        token.cancel();
    }
    let project = harness.open_project(std::path::Path::new(&path)).await?;
    let id = project.id.clone();
    Ok(ProjectView::new(project, Some(id.as_str())))
}

#[tauri::command]
pub async fn remove_project(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.harness().await?.forget_project(&id)
}

/// Register an existing directory as a project with a user-declared type; this is the only path that sets the
/// type, since `open_project` uses `touch` and preserves it, and the type decides which plugin layer loads.
#[tauri::command]
pub async fn create_project(
    path: String,
    kind: ProjectKind,
    state: State<'_, AppState>,
) -> Result<ProjectView, String> {
    let harness = state.harness().await?;
    let project = harness.create_project(std::path::Path::new(&path), kind.into(), None)?;
    let current = harness.current_project().map(|open| open.id);
    Ok(ProjectView::new(project, current.as_deref()))
}

/// Clone a repository and register it as a project. Progress goes through a `Channel`, not `emit`, which
/// would broadcast to every window. Dropping the stream is the only cancellation, so this function must hold
/// it alive for the whole clone; dropping it kills the `git` process group and cleans the partial directory.
#[tauri::command]
pub async fn clone_project(
    url: String,
    parent: String,
    name: Option<String>,
    depth: Option<u32>,
    kind: ProjectKind,
    on_progress: Channel<CloneProgress>,
    state: State<'_, AppState>,
) -> Result<ProjectView, String> {
    use futures::StreamExt;

    let harness = state.harness().await?;
    let request = CloneRequest {
        url: url.clone(),
        parent: std::path::PathBuf::from(parent),
        name,
        depth,
    };
    // Validate before opening the stream: a rejected `ext::` URL should be an immediate error, not a `Failed` event lost among progress lines.
    request.validate().map_err(|err| err.to_string())?;

    // Register the cancel token before opening the stream: a Tauri `Channel` never tells Rust the dialog closed,
    // so without an explicit cancel path the clone runs to completion after the user gave up.
    let cancel = CancellationToken::new();
    *state.cloning.lock() = Some(cancel.clone());

    let mut stream = pai_project::clone(request);
    // `CloneEvent` splits phase and progress across variants, but the UI needs each message to name its phase;
    // remember it here rather than making the UI fold state and hold a second source of truth.
    let mut phase = String::from("Đang chuẩn bị");
    let mut done: Option<std::path::PathBuf> = None;

    while let Some(event) = tokio::select! {
        biased;
        // Check cancellation first: dropping `stream` kills the `git` group and cleans up, so waiting for one
        // more event means downloading one more chunk nobody wants.
        _ = cancel.cancelled() => None,
        event = stream.next() => event,
    } {
        let progress = match event {
            CloneEvent::Phase { label } => {
                phase = label;
                CloneProgress {
                    phase: phase.clone(),
                    percent: None,
                    line: None,
                    finished: false,
                    path: None,
                    error: None,
                }
            }
            CloneEvent::Progress { label, percent } => {
                phase = label;
                CloneProgress {
                    phase: phase.clone(),
                    percent: Some(percent),
                    line: None,
                    finished: false,
                    path: None,
                    error: None,
                }
            }
            CloneEvent::Line { text } => CloneProgress {
                phase: phase.clone(),
                percent: None,
                line: Some(text),
                finished: false,
                path: None,
                error: None,
            },
            CloneEvent::Done { path } => {
                done = Some(path.clone());
                CloneProgress {
                    phase: "Xong".into(),
                    percent: Some(100),
                    line: None,
                    finished: true,
                    path: Some(path.display().to_string()),
                    error: None,
                }
            }
            CloneEvent::Failed { message } => CloneProgress {
                phase: phase.clone(),
                percent: None,
                line: None,
                finished: true,
                path: None,
                error: Some(message),
            },
        };
        let failed = progress.error.clone();
        // Losing the channel is real (the window closed), but it is no reason to abandon a running clone.
        if let Err(err) = on_progress.send(progress) {
            tracing::debug!("could not send clone progress: {err}");
        }
        if let Some(message) = failed {
            state.cloning.lock().take();
            return Err(message);
        }
    }

    state.cloning.lock().take();
    if cancel.is_cancelled() {
        return Err("đã huỷ bản clone".to_string());
    }

    let path = done.ok_or_else(|| {
        // The stream ended with neither `Done` nor `Failed`; `pai-project`'s contract says that cannot happen, so silence would be the worst response.
        "bản clone kết thúc mà không báo kết quả".to_string()
    })?;

    let project = harness.create_project(&path, kind.into(), Some(&url))?;
    let current = harness.current_project().map(|open| open.id);
    Ok(ProjectView::new(project, current.as_deref()))
}

/// Cancel a running clone, or do nothing if there is none; cancelling just as a clone finishes is a real race,
/// and an error dialog there would complain about something that went fine.
#[tauri::command]
pub fn cancel_clone(state: State<'_, AppState>) {
    if let Some(token) = state.cloning.lock().take() {
        token.cancel();
    }
}

/// Close the open project and fall back to plain conversation, cancelling running turns first for the same
/// reason as `open_project`.
#[tauri::command]
pub async fn close_project(state: State<'_, AppState>) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    for (_, token) in state.running.lock().drain() {
        token.cancel();
    }
    harness.close_project().await;
    list_projects(state).await
}

/// Change a project's type. The type is set once at registration and deliberately preserved by
/// `open_project`, so without this command a mis-typed directory is a dead end with no on-screen explanation.
#[tauri::command]
pub async fn set_project_kind(
    id: String,
    kind: ProjectKind,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    // Cancel running turns: changing the type rebuilds the plugin layer under them.
    for (_, token) in state.running.lock().drain() {
        token.cancel();
    }
    harness.set_project_kind(&id, kind.into()).await?;
    list_projects(state).await
}

/// Cap on entries returned for one directory: `node_modules` has tens of thousands, and an overlong list
/// should not cross the IPC bridge in the first place.
const MAX_ENTRIES: usize = 500;

/// One level of the project tree. Only one: recursing on open would read `node_modules` and `.git` for someone
/// who wanted a single folder, so each expansion is a call. Hidden files are not filtered -- `.gitignore`,
/// `.env` and `.github` are files people actually open, and a self-hiding tree makes users doubt their own disk.
#[tauri::command]
pub async fn list_dir(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::protocol::DirEntryView>, String> {
    use std::path::Path;

    let harness = state.harness().await?;
    // No project, no tree. Empty rather than an error: closing a project with the panel open is legitimate.
    let Some(open) = harness.current_project() else {
        return Ok(Vec::new());
    };

    // Read only inside the project directory; the UI sends a string, so compare after `canonicalize` or `..`
    // and symlinks turn this into a whole-disk reader.
    let root = Path::new(&open.path)
        .canonicalize()
        .map_err(|err| format!("không đọc được thư mục dự án: {err}"))?;
    let target = Path::new(&path)
        .canonicalize()
        .map_err(|err| format!("không mở được {path}: {err}"))?;
    if !target.starts_with(&root) {
        return Err("đường dẫn nằm ngoài thư mục dự án".to_string());
    }

    let mut entries: Vec<crate::protocol::DirEntryView> = std::fs::read_dir(&target)
        .map_err(|err| format!("không đọc được {}: {err}", target.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            // `file_type` is cheaper than `metadata` and does not follow symlinks; expanding one is stopped by the `starts_with` check above.
            let is_dir = entry.file_type().ok()?.is_dir();
            Some(crate::protocol::DirEntryView {
                name,
                path: entry.path().display().to_string(),
                is_dir,
            })
        })
        .collect();

    // Directories first, then case-insensitive by name: `read_dir` order is filesystem order, which is no order at all to a reader.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries.truncate(MAX_ENTRIES);
    Ok(entries)
}
