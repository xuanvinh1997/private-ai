//! Core state the settings screen reads: the sandbox and the installed hooks, both read-only. The sandbox is
//! a platform capability, not an option, and a toggle would promise what the OS will not give; hooks change
//! through the config file, so showing the truth beats a form that cannot save.

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

    // No provider means no sandbox at all, which must be stated rather than treated as an error: `pai-sandbox`
    // legitimately falls back to `Unconfined` on platforms it cannot confine.
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

/// Installed hooks, read from the layered configuration tree rather than `pai-hooks`, which discards config
/// after loading -- and the tree also knows which layer declared each row.
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
    // The last layer to touch a row has the final word, so it is the origin worth recording; absent means built-in.
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

/// The real patch file path, honouring `PAI_DATA_DIR`: the hooks screen only allows manual editing, so a
/// hard-coded path would send anyone with that variable set to edit a file nobody reads.
#[tauri::command]
pub async fn hook_config_path(state: State<'_, AppState>) -> Result<String, String> {
    let harness = state.harness().await?;
    Ok(harness.patch_path().display().to_string())
}
