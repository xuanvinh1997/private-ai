//! Attaching files to a message: the UI supplies paths, the core says whether they are usable.
//! Only this side can see the disk, so a TypeScript `startsWith` would pass symlinks and case variants that
//! `read` later refuses -- and refusing before send saves a whole model turn to deliver the same bad news.

use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::AppState;

/// A dropped or picked path, after the core has looked at the disk.
#[derive(Serialize)]
pub struct Attachment {
    /// The path exactly as the UI sent it, and what gets inserted into the composer when `error` is `None`;
    /// the resolved form is not returned, since Windows verbatim prefixes make a message unreadable.
    pub path: String,
    /// `None` means usable; the string is a user-readable sentence that already names the file.
    pub error: Option<String>,
}

/// Filter a batch of paths; one bad path never discards the batch. The whole batch fails in exactly one case,
/// no open project, which is not about any single file.
#[tauri::command]
pub async fn resolve_attachments(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<Attachment>, String> {
    let harness = state.harness().await?;
    let workspace = harness
        .workspace()
        .ok_or_else(|| "Chưa mở dự án, nên chưa có thư mục nào để đính kèm tệp vào.".to_string())?;
    // Resolve the root once, and resolve both sides: comparing a followed symlink against an unfollowed path
    // is how an in-project file gets reported as outside.
    let root = workspace.canonicalize().map_err(|err| {
        format!(
            "Không đọc được thư mục dự án {}: {err}",
            workspace.display()
        )
    })?;

    Ok(paths
        .into_iter()
        .map(|path| {
            let error = check(Path::new(&path), &root).err();
            Attachment { path, error }
        })
        .collect())
}

/// The name shown in an error: the file name, not the full path, which would push the reason off the line.
fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn check(path: &Path, root: &Path) -> Result<(), String> {
    let resolved = path
        .canonicalize()
        .map_err(|_| format!("Không tìm thấy {} trên đĩa.", name(path)))?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "{} nằm ngoài thư mục dự án, nên trợ lý không đọc được nó.",
            name(path)
        ));
    }
    Ok(())
}
