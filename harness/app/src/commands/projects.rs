//! Danh sách dự án và đổi dự án.

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

/// Đổi dự án.
///
/// Nặng ở phía lõi — nó tháo và cắm lại cả một nhánh plugin — nên huỷ mọi lượt đang chạy
/// trước: một lượt đang giữa chừng khi tool dưới chân nó bị gỡ ra sẽ hỏng theo cách không
/// giải thích được cho ai.
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

/// Ghi nhận một thư mục có sẵn thành dự án, với loại do người dùng nói ra.
///
/// Khác `open_project` ở đúng một chỗ và chỗ đó quan trọng: `open_project` dùng `touch`,
/// vốn **giữ nguyên** loại của một dự án đã có. Đây là đường duy nhất đặt loại, vì loại là
/// thứ quyết định tầng plugin nào được cắm — và một thư mục âm thầm đổi loại vì người dùng
/// mở lại nó là một tập tool đổi dưới chân họ.
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

/// Clone một repo rồi ghi nhận nó thành dự án.
///
/// Tiến trình đi qua `Channel` chứ không qua `emit`, cùng lý do như luồng token: `emit`
/// phát tới mọi cửa sổ và không có đường ghép sự kiện với lời gọi đã sinh ra nó.
///
/// **Thả luồng là huỷ**, và không có hàm huỷ nào khác — nên hàm này phải giữ luồng sống
/// suốt bản clone. Người dùng bấm huỷ ở giao diện thì lệnh này bị bỏ dở, luồng bị thả, và
/// `pai-project` giết cả nhóm tiến trình `git` rồi dọn thư mục tải dở.
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
    // Kiểm trước khi mở luồng: một URL `ext::` bị từ chối phải là một lỗi trả về ngay, chứ
    // không phải một sự kiện `Failed` lẫn giữa các dòng tiến trình.
    request.validate().map_err(|err| err.to_string())?;

    // Đăng ký token huỷ **trước** khi mở luồng. Một `Channel` của Tauri không nói cho
    // phía Rust biết người dùng đã đóng hộp thoại, nên nếu không có đường huỷ tường minh
    // thì bản clone chạy tiếp tới cùng sau khi họ đã bấm Huỷ và đi làm việc khác.
    let cancel = CancellationToken::new();
    *state.cloning.lock() = Some(cancel.clone());

    let mut stream = pai_project::clone(request);
    // `CloneEvent` mang pha và tiến trình ở hai biến thể rời nhau, còn giao diện cần mỗi
    // bản tin tự nói được nó thuộc pha nào. Nhớ pha ở đây, một chỗ, thay vì bắt giao diện
    // gấp trạng thái — giao diện gấp trạng thái là giao diện có hai nguồn sự thật.
    let mut phase = String::from("Đang chuẩn bị");
    let mut done: Option<std::path::PathBuf> = None;

    while let Some(event) = tokio::select! {
        biased;
        // Nhánh huỷ đứng trước: thả `stream` là giết cả nhóm tiến trình `git` và dọn thư
        // mục tải dở, nên chờ thêm một sự kiện nữa trước khi kiểm là chờ thêm một mảnh
        // tải về không ai cần.
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
        // Giao diện mất kết nối kênh là chuyện có thật (cửa sổ đóng giữa chừng); nó không
        // phải lý do để bỏ dở bản clone đang chạy.
        if let Err(err) = on_progress.send(progress) {
            tracing::debug!("không gửi được tiến trình clone: {err}");
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
        // Luồng kết thúc mà không có `Done` lẫn `Failed`: hợp đồng của `pai-project` nói
        // điều này không xảy ra, nên nếu nó xảy ra thì im lặng là cách tệ nhất.
        "bản clone kết thúc mà không báo kết quả".to_string()
    })?;

    let project = harness.create_project(&path, kind.into(), Some(&url))?;
    let current = harness.current_project().map(|open| open.id);
    Ok(ProjectView::new(project, current.as_deref()))
}

/// Huỷ bản clone đang chạy. Không có bản nào thì đây là một lệnh không làm gì.
///
/// Không báo lỗi khi không có gì để huỷ: người dùng bấm Huỷ đúng lúc bản clone vừa xong là
/// một cuộc đua có thật, và một hộp lỗi ở đó nói về một chuyện đã kết thúc tốt đẹp.
#[tauri::command]
pub fn cancel_clone(state: State<'_, AppState>) {
    if let Some(token) = state.cloning.lock().take() {
        token.cancel();
    }
}

/// Đóng dự án đang mở, quay về trò chuyện thuần tuý.
///
/// Huỷ mọi lượt đang chạy trước, cùng lý do như `open_project`: một lượt đang giữa chừng
/// khi tool dưới chân nó bị gỡ ra sẽ hỏng theo cách không giải thích được cho ai.
#[tauri::command]
pub async fn close_project(state: State<'_, AppState>) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    for (_, token) in state.running.lock().drain() {
        token.cancel();
    }
    harness.close_project().await;
    list_projects(state).await
}

/// Đổi loại của một dự án.
///
/// Loại được đặt một lần lúc ghi nhận và `open_project` cố ý giữ nguyên nó, nên không có
/// lệnh này thì một thư mục vào nhầm loại là một ngõ cụt: người dùng chỉ thấy trợ lý nói
/// nó không có tool nào để đọc tệp, và không có gì trên màn hình nói vì sao.
#[tauri::command]
pub async fn set_project_kind(
    id: String,
    kind: ProjectKind,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    // Huỷ lượt đang chạy: đổi loại là tháo và cắm lại cả tầng plugin, và một lượt đang
    // giữa chừng khi tool dưới chân nó bị gỡ ra sẽ hỏng không giải thích được cho ai.
    for (_, token) in state.running.lock().drain() {
        token.cancel();
    }
    harness.set_project_kind(&id, kind.into()).await?;
    list_projects(state).await
}

/// Trần số mục trả về cho **một** thư mục.
///
/// Một thư mục `node_modules` có hàng chục nghìn mục, và vẽ hết chúng ra không giúp ai
/// tìm được gì — nó chỉ làm treo cột bên phải. Cắt ở đây chứ không ở giao diện: một danh
/// sách quá dài không nên đi qua cầu IPC ngay từ đầu.
const MAX_ENTRIES: usize = 500;

/// Một tầng của cây thư mục dự án.
///
/// **Chỉ một tầng.** Đọc đệ quy cả cây lúc mở bảng là đọc cả `node_modules` và `.git` cho
/// một người dùng chỉ định mở một thư mục — nên mỗi lần bung một nhánh là một lời gọi, và
/// nhánh chưa bung thì chưa tốn gì.
///
/// Không lọc bỏ tệp ẩn. `.gitignore`, `.env`, `.github` là những tệp người ta thật sự cần
/// mở, và một cây tự giấu bớt là một cây khiến người dùng kết luận tệp của họ không có ở
/// đó. `.git` cũng vẫn hiện, nhưng vì cây đọc theo từng tầng nên nó không tốn gì cho tới
/// khi có người cố ý bung nó ra.
#[tauri::command]
pub async fn list_dir(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::protocol::DirEntryView>, String> {
    use std::path::Path;

    let harness = state.harness().await?;
    // Không có dự án thì không có cây. Trả rỗng chứ không lỗi: đóng dự án trong lúc bảng
    // còn mở là một thao tác hợp lệ, và một hộp lỗi ở đó là phạt người dùng vì đã đóng.
    let Some(open) = harness.current_project() else {
        return Ok(Vec::new());
    };

    // Chỉ đọc **trong** thư mục dự án. Giao diện gửi lên một chuỗi, và một chuỗi từ giao
    // diện là thứ duy nhất ngăn lệnh này thành một đường đọc cả ổ đĩa. So sau khi
    // `canonicalize` để `..` và symlink không lách qua được.
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
            // `file_type` rẻ hơn `metadata` và không đi theo symlink — một symlink trỏ ra
            // ngoài dự án vì thế hiện ra như một mục thường, và bung nó ra thì phép so
            // `starts_with` ở trên chặn lại.
            let is_dir = entry.file_type().ok()?.is_dir();
            Some(crate::protocol::DirEntryView {
                name,
                path: entry.path().display().to_string(),
                is_dir,
            })
        })
        .collect();

    // Thư mục trước, rồi xếp theo tên không phân biệt hoa thường. Thứ tự của `read_dir`
    // là thứ tự của hệ tệp, tức là không có thứ tự nào cả với người đọc.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries.truncate(MAX_ENTRIES);
    Ok(entries)
}
