//! MCP servers: viewing status, adding new ones, and the built-in catalogue.

use std::collections::BTreeMap;

use pai_mcp::{CATALOG, Mcp, McpConfig, McpTransport, ServerConfig, ServerState};
use tauri::State;

use crate::AppState;
use crate::harness::Harness;
use crate::protocol::{McpCatalogEntry, McpEnvVar, McpServerInputWire, McpServerView};

/// One line to display: the full command, or the URL.
fn target(transport: &McpTransport) -> String {
    match transport {
        McpTransport::Stdio { command, args, .. } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            }
        }
        McpTransport::Http { url, .. } => url.clone(),
    }
}

fn transport_name(transport: &McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio { .. } => "stdio",
        McpTransport::Http { .. } => "http",
    }
}

/// Merge the store with the hub's live state: the store knows what the user wants, the hub what is actually
/// happening. A disabled server never appears in `status()`, so `"disabled"` is inferred from the store.
async fn views(harness: &Harness) -> Result<Vec<McpServerView>, String> {
    let store = harness
        .ctx
        .require::<McpConfig>()
        .map_err(|err| err.to_string())?;
    let hub = harness
        .ctx
        .require::<Mcp>()
        .map_err(|err| err.to_string())?;

    let configured = pai_mcp::merge(
        harness.mcp_rows.clone(),
        store.list().map_err(|err| err.to_string())?,
    );
    let live: BTreeMap<String, pai_mcp::ServerStatus> = hub
        .status()
        .into_iter()
        .map(|status| (status.name.clone(), status))
        .collect();

    Ok(configured
        .into_iter()
        .map(|config| {
            let status = live.get(&config.name);
            let state = match (config.enabled, status.map(|item| &item.state)) {
                (false, _) => "disabled",
                (true, Some(ServerState::Ready { .. })) => "connected",
                (true, Some(ServerState::Connecting)) => "connecting",
                (true, _) => "failed",
            };
            McpServerView {
                transport: transport_name(&config.transport).to_string(),
                target: target(&config.transport),
                state: state.to_string(),
                tools: status.map(|item| item.tools.clone()).unwrap_or_default(),
                error: status.and_then(|item| item.error.clone()),
                enabled: config.enabled,
                name: config.name,
            }
        })
        .collect())
}

/// Apply configuration to the running hub. The only path.
async fn reapply(harness: &Harness) -> Result<(), String> {
    let store = harness
        .ctx
        .require::<McpConfig>()
        .map_err(|err| err.to_string())?;
    let hub = harness
        .ctx
        .require::<Mcp>()
        .map_err(|err| err.to_string())?;
    let outcomes = pai_mcp::apply(&hub, &store, &harness.mcp_rows)
        .await
        .map_err(|err| err.to_string())?;
    for (name, outcome) in outcomes {
        if let Err(err) = outcome {
            // One broken server does not fail the command; the UI reads the reason from `status()` right after.
            tracing::warn!(server = %name, "could not attach: {err}");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerView>, String> {
    let harness = state.harness().await?;
    views(&harness).await
}

/// The built-in catalogue, with prerequisites checked on this machine before the user clicks; without the
/// check they wait twenty seconds for a `failed` server that explains nothing.
#[tauri::command]
pub async fn mcp_catalog(state: State<'_, AppState>) -> Result<Vec<McpCatalogEntry>, String> {
    let _ = state;
    Ok(CATALOG
        .iter()
        .map(|entry| McpCatalogEntry {
            id: entry.id.to_string(),
            name: entry.name.to_string(),
            summary: entry.summary.to_string(),
            command: entry.command.to_string(),
            args: entry.args.iter().map(|arg| arg.to_string()).collect(),
            env: entry
                .env
                .iter()
                .map(|var| McpEnvVar {
                    key: var.key.to_string(),
                    label: var.label.to_string(),
                    required: var.required,
                    secret: var.secret,
                })
                .collect(),
            homepage: entry.homepage.to_string(),
            requires: entry
                .requires
                .iter()
                .filter(|tool| !on_path(tool))
                .map(|tool| tool.to_string())
                .collect(),
            url: entry.url.map(|url| url.to_string()),
        })
        .collect())
}

/// Whether this command is on `PATH`; `which`/`where` know about shell aliases, Windows `PATHEXT` and the
/// execute bit, which a hand-rolled loop does not.
fn on_path(tool: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(finder)
        .arg(tool)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// One row after applying; absence means the store accepted it but `merge` did not return it, which would show as a vanished row.
fn one(views: Vec<McpServerView>, name: &str) -> Result<McpServerView, String> {
    views
        .into_iter()
        .find(|item| item.name == name)
        .ok_or_else(|| format!("đã lưu `{name}` nhưng không đọc lại được nó"))
}

#[tauri::command]
pub async fn save_mcp_server(
    input: McpServerInputWire,
    state: State<'_, AppState>,
) -> Result<McpServerView, String> {
    let harness = state.harness().await?;
    let transport = match input.transport.as_str() {
        "stdio" => McpTransport::Stdio {
            command: input.command,
            args: input.args,
            env: input.env,
            cwd: input.cwd.map(std::path::PathBuf::from),
        },
        "http" => McpTransport::Http {
            url: input.url,
            headers: input.headers,
        },
        other => return Err(format!("transport không hợp lệ: `{other}`")),
    };
    let config = ServerConfig {
        name: input.name,
        transport,
        enabled: input.enabled,
        connect_timeout_secs: pai_mcp::config::CONNECT_TIMEOUT.as_secs(),
        max_retries: pai_mcp::config::DEFAULT_MAX_RETRIES,
    };
    let store = harness
        .ctx
        .require::<McpConfig>()
        .map_err(|err| err.to_string())?;
    let name = config.name.clone();
    store.save(config).map_err(|err| err.to_string())?;
    reapply(&harness).await?;
    one(views(&harness).await?, &name)
}

#[tauri::command]
pub async fn remove_mcp_server(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let harness = state.harness().await?;
    let store = harness
        .ctx
        .require::<McpConfig>()
        .map_err(|err| err.to_string())?;
    if !store.remove(&name).map_err(|err| err.to_string())? {
        // Servers declared in `patch.yaml` are not in the store, so say where to edit instead of reporting "not found".
        return Err(format!(
            "`{name}` không có trong kho — nếu nó được khai trong patch.yaml thì phải sửa ở đó"
        ));
    }
    reapply(&harness).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_mcp_enabled(
    name: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let harness = state.harness().await?;
    let store = harness
        .ctx
        .require::<McpConfig>()
        .map_err(|err| err.to_string())?;
    store
        .set_enabled(&name, enabled)
        .map_err(|err| err.to_string())?;
    reapply(&harness).await?;
    Ok(())
}

#[tauri::command]
pub async fn reload_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerView>, String> {
    let harness = state.harness().await?;
    reapply(&harness).await?;
    views(&harness).await
}
