//! Attaching files to a message: the UI supplies paths, the core decides where each one has to live.
//! Only this side can see the disk, so a TypeScript `startsWith` would pass symlinks and case variants that
//! `read` later refuses -- and refusing before send saves a whole model turn to deliver the same bad news.
//!
//! A file already inside the project keeps its own path. A file from anywhere else is copied into the
//! session's own attachment folder, because attaching to a conversation is not the same act as adding a file
//! to the project: the user's folder stays exactly as they left it, and the copy lands in a directory the
//! read tool is granted, so the model can open what was attached.

use std::collections::HashMap;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use pai_rag::{Docs, IngestFile, IngestStage, needs_extraction};
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::harness::Harness;

/// Past this size a copy stops being an attachment and becomes a second library on disk; nothing downstream
/// could read such a file into a turn anyway.
const TOI_DA: u64 = 100 * 1024 * 1024;

/// How many `ten-2.txt` variants to try before giving up; a folder that full is a bug, not a busy user.
const TOI_DA_TRUNG_TEN: u32 = 100;

/// A dropped or picked path, after the core has looked at the disk.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The path that gets inserted into the composer when `error` is `None`: the original for a file inside
    /// the project, exactly as the UI sent it, since Windows verbatim prefixes make a message unreadable;
    /// the path of the copy for a file from outside.
    pub path: String,
    /// `None` means usable; the string is a user-readable sentence that already names the file.
    pub error: Option<String>,
    /// Whether the file went through the library instead of staying a plain path: true for the PDFs, images
    /// and DOCX files `read` cannot open, which the model reaches with `attachment.search` and
    /// `attachment.read`.
    pub extracted: bool,
}

/// Place a batch of paths; one bad path never discards the batch. The whole batch fails in exactly one case,
/// no open project, which is not about any single file.
#[tauri::command]
pub async fn resolve_attachments(
    state: State<'_, AppState>,
    paths: Vec<String>,
    session_id: String,
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
    let kho = harness.session_attachments(&session_id)?;

    let mut placed: Vec<Attachment> = paths
        .into_iter()
        .map(|path| match place(Path::new(&path), &root, &kho) {
            Ok(placed) => Attachment {
                path: placed,
                error: None,
                extracted: false,
            },
            Err(error) => Attachment {
                path,
                error: Some(error),
                extracted: false,
            },
        })
        .collect();

    trich_xuat(&state, &harness, &mut placed).await;
    Ok(placed)
}

/// Hand the files `read` cannot open to the library that owns the extractors -- pdfium, the DOCX reader, OCR
/// through the vision role. Nothing here re-implements any of that; the whole point is that one path exists.
/// Failures land on the file they belong to, so the rest of the batch still attaches.
async fn trich_xuat(state: &State<'_, AppState>, harness: &Harness, placed: &mut [Attachment]) {
    // No upload list to tick boxes on here, so these follow the saved OCR setting.
    let can_doc: Vec<IngestFile> = placed
        .iter()
        .filter(|entry| entry.error.is_none() && needs_extraction(Path::new(&entry.path)))
        .map(|entry| IngestFile::new(PathBuf::from(&entry.path)))
        .collect();
    if can_doc.is_empty() {
        return;
    }

    let Some(docs) = harness.ctx.get::<Docs>() else {
        // A document project mounts its library over the user's own folder, not over the attachment folder.
        for entry in placed.iter_mut().filter(|entry| entry.error.is_none()) {
            if needs_extraction(Path::new(&entry.path)) {
                entry.error = Some(format!(
                    "{} cần được trích xuất, nhưng dự án này chưa có thư viện tài liệu.",
                    name(Path::new(&entry.path))
                ));
            }
        }
        return;
    };

    // Vectors are optional -- without an embedding model the text is still stored and still found by keyword --
    // so a sidecar that will not start is a warning, not a refusal to attach.
    if let Err(err) = state.qdrant.ensure().await {
        tracing::warn!("attachment extraction without the vector store: {err}");
    }

    let mut hong: HashMap<String, String> = HashMap::new();
    let mut xong: Vec<String> = Vec::new();
    let mut stream = docs.ingest(can_doc);
    while let Some(event) = stream.next().await {
        match event.stage {
            IngestStage::Stored => xong.push(event.path),
            IngestStage::Failed | IngestStage::Skipped => {
                hong.insert(
                    event.path.clone(),
                    event.error.unwrap_or_else(|| {
                        format!("Không đọc được nội dung {}.", name(Path::new(&event.path)))
                    }),
                );
            }
            _ => {}
        }
    }
    drop(stream);

    for entry in placed.iter_mut() {
        if let Some(err) = hong.remove(&entry.path) {
            entry.error = Some(err);
        } else if xong.contains(&entry.path) {
            entry.extracted = true;
        }
    }
}

/// The name shown in an error: the file name, not the full path, which would push the reason off the line.
fn name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Where the composer should point at this file, once it is somewhere the model may read.
fn place(path: &Path, root: &Path, kho: &Path) -> Result<String, String> {
    let resolved = path
        .canonicalize()
        .map_err(|_| format!("Không tìm thấy {} trên đĩa.", name(path)))?;
    // Inside the project already: leave it where it is, since a copy would only start drifting from the file
    // the user is still editing.
    if resolved.starts_with(root) {
        return Ok(path.display().to_string());
    }
    let meta = std::fs::metadata(&resolved)
        .map_err(|err| format!("Không đọc được {}: {err}", name(path)))?;
    if meta.is_dir() {
        return Err(format!(
            "{} là thư mục; hãy đính kèm từng tệp trong đó, hoặc mở nó làm dự án.",
            name(path)
        ));
    }
    if meta.len() > TOI_DA {
        return Err(format!(
            "{} nặng {}, vượt giới hạn {} cho tệp đính kèm.",
            name(path),
            co(meta.len()),
            co(TOI_DA)
        ));
    }
    sao_chep(&resolved, &meta, kho).map(|copy| copy.display().to_string())
}

/// Copy the file into the session's folder and answer with the copy's path. Attaching the same file twice
/// reuses the first copy instead of growing a pile of `bao-cao-2.pdf`.
fn sao_chep(source: &Path, meta: &Metadata, kho: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(kho)
        .map_err(|err| format!("Không tạo được thư mục đính kèm {}: {err}", kho.display()))?;
    // `source` is canonical, so a file name always exists and can never be `..` or a separator.
    let file_name = source
        .file_name()
        .map(Path::new)
        .ok_or_else(|| format!("{} không có tên tệp để đính kèm.", source.display()))?;

    for lan in 0..TOI_DA_TRUNG_TEN {
        let dich = kho.join(danh_so(file_name, lan));
        match std::fs::metadata(&dich) {
            // Free name: this is the copy.
            Err(_) => {
                std::fs::copy(source, &dich)
                    .map_err(|err| format!("Không sao chép được {}: {err}", name(source)))?;
                return Ok(dich);
            }
            // Same name and the same bytes: the user attached this file before, so point at that copy.
            // Lengths are compared first, so the byte walk only runs for a genuine collision.
            Ok(cu) if cu.len() == meta.len() && giong_nhau(source, &dich).unwrap_or(false) => {
                return Ok(dich);
            }
            Ok(_) => continue,
        }
    }
    Err(format!(
        "Đã có quá nhiều tệp tên {} trong phiên này.",
        name(source)
    ))
}

/// `bao-cao.pdf`, then `bao-cao-2.pdf`: the extension stays last so the file still opens by double-click.
fn danh_so(file_name: &Path, lan: u32) -> PathBuf {
    if lan == 0 {
        return file_name.to_path_buf();
    }
    let than = file_name.file_stem().unwrap_or(file_name.as_os_str());
    let mut ten = than.to_string_lossy().into_owned();
    ten.push_str(&format!("-{}", lan + 1));
    match file_name.extension() {
        Some(duoi) => PathBuf::from(format!("{ten}.{}", duoi.to_string_lossy())),
        None => PathBuf::from(ten),
    }
}

/// Byte comparison, streamed, so a 100 MB collision never lands two copies of the file in memory.
fn giong_nhau(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut a = std::io::BufReader::new(std::fs::File::open(a)?);
    let mut b = std::io::BufReader::new(std::fs::File::open(b)?);
    let (mut dem_a, mut dem_b) = ([0u8; 64 * 1024], [0u8; 64 * 1024]);
    loop {
        let n = a.read(&mut dem_a)?;
        b.read_exact(&mut dem_b[..n])?;
        if dem_a[..n] != dem_b[..n] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

/// Sizes in an error sentence, where `104857600` reads as noise.
fn co(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project file is attached where it lies, path untouched: the model reads the file the user is editing,
    /// not a snapshot of it.
    #[test]
    fn trong_du_an_giu_nguyen_duong_dan() {
        let du_an = tempfile::tempdir().unwrap();
        let kho = tempfile::tempdir().unwrap();
        let file = du_an.path().join("ghi-chu.md");
        std::fs::write(&file, b"xin chao").unwrap();

        let root = du_an.path().canonicalize().unwrap();
        let placed = place(&file, &root, kho.path()).unwrap();

        assert_eq!(placed, file.display().to_string());
        assert_eq!(std::fs::read_dir(kho.path()).unwrap().count(), 0);
    }

    /// The point of the change: a file from anywhere else is copied into the session folder, and the composer
    /// is handed the copy.
    #[test]
    fn ngoai_du_an_duoc_sao_chep() {
        let du_an = tempfile::tempdir().unwrap();
        let ngoai = tempfile::tempdir().unwrap();
        let phien = tempfile::tempdir().unwrap();
        let kho = phien.path().join("phien-1");
        let file = ngoai.path().join("bao-cao.pdf");
        std::fs::write(&file, b"noi dung").unwrap();

        let root = du_an.path().canonicalize().unwrap();
        let placed = place(&file, &root, &kho).unwrap();

        assert_eq!(placed, kho.join("bao-cao.pdf").display().to_string());
        assert_eq!(std::fs::read(&placed).unwrap(), b"noi dung");
    }

    /// Attaching the same file twice points at the one copy; attaching a different file of the same name gets
    /// its own, because overwriting would change what an earlier message in the conversation refers to.
    #[test]
    fn trung_ten_thi_dung_lai_hoac_danh_so() {
        let du_an = tempfile::tempdir().unwrap();
        let ngoai = tempfile::tempdir().unwrap();
        let phien = tempfile::tempdir().unwrap();
        let kho = phien.path().join("phien-1");
        let root = du_an.path().canonicalize().unwrap();

        let file = ngoai.path().join("bao-cao.pdf");
        std::fs::write(&file, b"noi dung").unwrap();
        let dau = place(&file, &root, &kho).unwrap();
        assert_eq!(place(&file, &root, &kho).unwrap(), dau);

        let khac = ngoai.path().join("khac/bao-cao.pdf");
        std::fs::create_dir_all(khac.parent().unwrap()).unwrap();
        std::fs::write(&khac, b"noi dung khac").unwrap();
        assert_eq!(
            place(&khac, &root, &kho).unwrap(),
            kho.join("bao-cao-2.pdf").display().to_string()
        );
    }

    /// Every refusal names the file, since the notice the user sees is this sentence and nothing else.
    #[test]
    fn tu_choi_thi_noi_ro_ly_do() {
        let du_an = tempfile::tempdir().unwrap();
        let ngoai = tempfile::tempdir().unwrap();
        let kho = tempfile::tempdir().unwrap();
        let root = du_an.path().canonicalize().unwrap();

        let thieu = place(Path::new("/khong/he/co/tep.txt"), &root, kho.path()).unwrap_err();
        assert!(thieu.contains("tep.txt"), "{thieu}");

        let thu_muc = ngoai.path().join("tai-lieu");
        std::fs::create_dir_all(&thu_muc).unwrap();
        let err = place(&thu_muc, &root, kho.path()).unwrap_err();
        assert!(err.contains("tai-lieu") && err.contains("thư mục"), "{err}");
    }
}
