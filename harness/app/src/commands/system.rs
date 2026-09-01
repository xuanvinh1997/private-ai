//! Trạng thái lõi mà màn hình cài đặt cần đọc: vòng giam, và hook đang cài.
//!
//! Cả hai đều **chỉ đọc**. Vòng giam không đổi được từ giao diện vì nó không phải một
//! tuỳ chọn — nó là năng lực của nền tảng, và một công tắc ở đó sẽ hứa một thứ hệ điều
//! hành không cho. Hook thì đổi được, nhưng bằng tệp cấu hình, và cho tới khi có lệnh ghi
//! thì màn hình đọc ra sự thật còn hơn dựng một biểu mẫu không lưu được.

use pai_sandbox::Sandbox;
use tauri::State;

use crate::AppState;
use crate::protocol::{HookRow, SandboxStatus};

#[tauri::command]
pub async fn sandbox_status(state: State<'_, AppState>) -> Result<SandboxStatus, String> {
    let harness = state.harness().await?;
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "khác"
    };

    // Không có provider nghĩa là **không có vòng giam nào**, và đó là câu phải nói ra chứ
    // không phải một lỗi: `pai-sandbox` lùi về `Unconfined` một cách hợp lệ trên nền tảng
    // nó không giam được, và người dùng cần biết mình đang ở trạng thái ấy.
    let Some(sandbox) = harness.ctx.get::<Sandbox>() else {
        return Ok(SandboxStatus {
            mode: "none".into(),
            reason: Some("chưa có vòng giam nào được cắm".into()),
            writable_roots: Vec::new(),
            platform: platform.into(),
        });
    };

    let enforcement = sandbox.enforcement();
    let roots = harness
        .workspace()
        .map(|dir| pai_sandbox::writable_roots(&pai_sandbox::Policy::workspace_write(dir)))
        .unwrap_or_default();

    Ok(SandboxStatus {
        mode: enforcement.label().to_string(),
        reason: enforcement.reason().map(str::to_string),
        writable_roots: roots
            .into_iter()
            .map(|dir| dir.display().to_string())
            .collect(),
        platform: platform.into(),
    })
}

/// Hook đang cài, đọc từ **cây cấu hình đã áp lớp**.
///
/// Đọc từ đó chứ không từ `pai-hooks` vì bản thân plugin không giữ lại cấu hình sau khi
/// cắm — và cây đã áp lớp còn biết thêm một thứ mà plugin không biết: **lớp nào đã khai
/// hàng này**. Người dùng sửa `patch.yaml` rồi không thấy hook chạy sẽ hỏi đúng câu đó.
#[tauri::command]
pub async fn list_hooks(state: State<'_, AppState>) -> Result<Vec<HookRow>, String> {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Row {
        hooks: Vec<pai_hooks::HookConfig>,
    }

    let harness = state.harness().await?;
    let Some(row) = harness.plugins.active().find(|row| row.plugin == "hooks") else {
        return Ok(Vec::new());
    };
    let parsed: Row = serde_json::from_value(row.config.clone()).map_err(|err| err.to_string())?;
    // Lớp cuối cùng chạm vào hàng này là lớp có tiếng nói cuối cùng, nên nó là nguồn gốc
    // đáng ghi. Vắng thì hàng đến từ bản dựng sẵn.
    let origin = harness
        .plugins
        .provenance
        .get(&row.id)
        .and_then(|layers| layers.last())
        .cloned()
        .unwrap_or_else(|| "nền (dựng sẵn)".to_string());

    Ok(parsed
        .hooks
        .into_iter()
        .map(|hook| HookRow {
            command: hook.command,
            tools: hook.tools,
            timeout_secs: hook.timeout_secs,
            origin: origin.clone(),
        })
        .collect())
}

/// Đường dẫn tệp vá thật, tôn trọng `PAI_DATA_DIR`.
///
/// Màn hình Hook chỉ cho sửa bằng tay, nên nó phải chỉ đúng tệp — một đường dẫn viết cứng
/// `~/.private-ai/patch.yaml` sẽ sai với mọi người đã đặt biến môi trường ấy, và họ sẽ sửa
/// một tệp không ai đọc.
#[tauri::command]
pub async fn hook_config_path(state: State<'_, AppState>) -> Result<String, String> {
    let harness = state.harness().await?;
    Ok(harness.patch_path().display().to_string())
}
