//! Thư viện tài liệu: nạp lên, liệt kê, xoá, và thử tìm.
//!
//! Không có tool nào của mô hình xuất hiện ở đây, và đó là có chủ ý: nạp và xoá tài liệu
//! là hành động của **người dùng**. Mô hình chỉ được `docs.search`, `docs.read`,
//! `docs.list` — nó đọc thư viện, nó không sắp xếp lại thư viện.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use pai_rag::{DocLibrary, Docs, Document};
use tauri::State;
use tauri::ipc::Channel;

use crate::AppState;
use crate::harness::Harness;
use crate::protocol::{DocumentHit, DocumentView, IngestProgress, LibraryStats};

/// Thư viện của dự án đang mở.
///
/// Vắng mặt là trạng thái **hợp lệ**: dự án mã nguồn không cắm `rag`. Câu trả lời nói ra
/// loại dự án, vì đó là thứ người dùng cần để hiểu — không phải một lỗi kỹ thuật.
fn library(harness: &Harness) -> Result<Arc<dyn DocLibrary>, String> {
    harness
        .ctx
        .get::<Docs>()
        .ok_or_else(|| "dự án đang mở không phải thư viện tài liệu".to_string())
}

fn view(doc: Document) -> DocumentView {
    DocumentView {
        id: doc.id,
        path: doc.origin,
        title: doc.title,
        format: doc.format.as_str().to_string(),
        bytes: doc.bytes,
        chunks: doc.chunks,
        embedded: doc.embedded,
        added_at: doc.added_at,
        error: doc.error,
    }
}

#[tauri::command]
pub async fn list_documents(state: State<'_, AppState>) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    Ok(library(&harness)?
        .documents()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(view)
        .collect())
}

#[tauri::command]
pub async fn library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let harness = state.harness().await?;
    let stats = library(&harness)?.stats().map_err(|err| err.to_string())?;
    Ok(LibraryStats {
        documents: stats.documents,
        chunks: stats.chunks,
        embedded_chunks: stats.embedded_chunks,
        embedder: stats.embedder,
        semantic_ready: stats.semantic_ready,
        reason: stats.reason,
    })
}

/// Nạp một mẻ tệp, phát tiến trình, rồi trả về **cả thư viện**.
///
/// Trả cả thư viện chứ không trả phần vừa thêm: nạp lại một tệp đã có sẽ cập nhật hàng cũ
/// thay vì tạo hàng mới, nên "phần vừa thêm" không phải một khái niệm đúng ở đây. Giao
/// diện thay nguyên bảng, và không có cách nào để hai bên lệch nhau.
///
/// Một tệp hỏng **không** làm hỏng cả mẻ. Đó là hợp đồng của `pai-rag` và nó phải đi
/// nguyên vẹn tới màn hình: người dùng thả hai mươi tệp, một tệp là PDF hỏng, và mười chín
/// tệp kia vẫn phải vào.
#[tauri::command]
pub async fn add_documents(
    paths: Vec<String>,
    on_progress: Channel<IngestProgress>,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentView>, String> {
    let harness = state.harness().await?;
    let library = library(&harness)?;
    let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();

    let mut stream = library.ingest(files);
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
            // Cửa sổ đóng giữa chừng không phải lý do bỏ dở việc nạp: những tệp đã sao
            // vào kho sẽ nằm đó dở dang nếu ta dừng ở đây.
            tracing::debug!("không gửi được tiến trình nạp: {err}");
        }
    }
    drop(stream);

    Ok(library
        .documents()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(view)
        .collect())
}

#[tauri::command]
pub async fn remove_document(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    library(&harness)?
        .remove(&id)
        .map_err(|err| err.to_string())
}

/// Thử tìm, để người dùng kiểm chứng thư viện **trước khi** hỏi trợ lý.
///
/// Trả về nguyên văn đoạn khớp. Giao diện phải hiện nó như một **trích dẫn**, không như
/// lời của ứng dụng: đây là chữ do người ngoài viết, và cùng ranh giới tin cậy mà ba tool
/// `docs.*` phải giữ cũng áp dụng ở đây.
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
