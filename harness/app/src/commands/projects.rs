//! Danh sách dự án, đổi dự án, và hai lệnh duyệt cây tệp.

use pai_project::{CloneEvent, CloneRequest};
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::protocol::{CloneProgress, ProjectKind, ProjectView};

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectView>, String> {
    let harness = state.harness().await?;
    let current = harness.current_project().id;
    Ok(harness
        .projects()?
        .into_iter()
        .map(|project| ProjectView::new(project, &current))
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
    Ok(ProjectView::new(project, &id))
}

#[tauri::command]
pub async fn remove_project(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.harness().await?.forget_project(&id)
}

#[tauri::command]
pub async fn list_tree(
    path: Option<String>,
    depth: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<pai_project::TreeEntry>, String> {
    let harness = state.harness().await?;
    let root = harness.workspace();
    let at = path.map(std::path::PathBuf::from);
    pai_project::list_tree(&root, at.as_deref(), depth.unwrap_or(1)).map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn read_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<pai_project::FileView, String> {
    let harness = state.harness().await?;
    pai_project::read_file(&harness.workspace(), std::path::Path::new(&path))
        .map_err(|err| err.to_string())
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
    let current = harness.current_project().id;
    Ok(ProjectView::new(project, &current))
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
    let current = harness.current_project().id;
    Ok(ProjectView::new(project, &current))
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
