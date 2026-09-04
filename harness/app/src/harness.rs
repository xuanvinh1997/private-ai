//! Building the plugin tree. What this application consists of lives in one function that reads as a list:
//! adding a capability is adding a line, and nothing else needs to know. The list will move to layered config
//! files later; until then it stays in code, because an unused config loader is an unverified one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pai_agent::{
    AgentPlugin, CompactionPlugin, Driver, Prompt, SkillsPlugin, SubagentPlugin, SystemPrompt,
};
use pai_core::{Composed, Context, Layer, Plugin, PluginCatalog, Row, compose};
use pai_fs::FsPlugin;
use pai_hooks::{HookConfig, HooksPlugin};
use pai_index::IndexPlugin;
use pai_llm::{AdapterRegistry, LlmAdapter, OllamaAdapter, ProviderKind};
use pai_lsp::LspPlugin;
use pai_mcp::{ExposeOptions, McpPlugin, ServerConfig, token_path};
use pai_project::{Project, ProjectKind, ProjectStore, SqliteProjectStore};
use pai_providers::{
    DB_FILE, ProviderInput, ProviderRuntime, ProviderStore, Providers, Role, SqliteProviderStore,
};
use pai_asr::Asr;
use pai_rag::{RagPlugin, purge_library};
use pai_sandbox::SandboxPlugin;
use pai_session::{SessionScope, SessionService, SessionStore, SqliteSessionStore};
use pai_shell::ShellPlugin;
use pai_terminal::TerminalPlugin;
use pai_tools::{ToolPipeline, ToolRegistry, Tools, ToolsPlugin};

use crate::llm::ActiveLlm;
use crate::rag_config::RagConfigFile;

/// The self-introduction at the head of every prompt.
const IDENTITY: &str = "\
Bạn là trợ lý lập trình chạy trên máy của người dùng. Bạn đọc và sửa mã nguồn trong thư \
mục làm việc, chạy lệnh khi cần, và nói tiếng Việt.

Trước khi sửa một tệp, hãy đọc nó. Trước khi kết luận, hãy kiểm chứng. Khi một việc \
không làm được, hãy nói ra thay vì làm một việc gần giống.";

pub struct Harness {
    pub ctx: Context,
    /// The context window the compaction plugin uses as its threshold, not the model's `context_window`, so
    /// the UI pressure bar fills exactly when compaction is about to run.
    pub context_window: usize,
    pub sessions: SessionService,
    pub driver: Arc<Driver>,
    /// The layered tree, kept so we can answer what the running build consists of.
    pub plugins: Composed,
    /// Application plugin scopes in load order; they live as long as the process.
    scopes: Vec<Context>,
    /// Project plugin scopes, torn down and rebuilt on every project switch.
    project_scopes: tokio::sync::Mutex<Vec<Context>>,
    projects: Arc<dyn ProjectStore>,
    current: parking_lot::Mutex<Option<Project>>,
    /// Pointer to the active provider; everything that talks to the model holds this, never a copy.
    pub llm: Arc<ActiveLlm>,
    /// The configuration file the native RAG library reads whenever a provider role changes.
    pub rag_config: Arc<RagConfigFile>,
    /// The one speech recognizer in the process. Both document-library mounts and the composer's
    /// microphone hold this same handle, so the model is loaded once and switched in one place.
    pub asr: Asr,
    pub providers: Arc<ProviderRuntime>,
    /// MCP servers declared in the config row (`patch.yaml`), kept so every reload can pass them back in:
    /// `pai_mcp::apply` removes any server missing from the list it receives.
    pub mcp_rows: Vec<ServerConfig>,
    /// Enough to rebuild the project layer; keeping the whole `Config` would carry a `workspace` field that
    /// stops being true after the first project switch.
    rebuild: Rebuild,
}

/// What `open_project` needs to rebuild the project layer.
struct Rebuild {
    ctx: Context,
    config: Config,
    llm: Arc<ActiveLlm>,
    rag_config: Arc<RagConfigFile>,
    asr: Asr,
    sessions: SessionService,
    composed: Composed,
}

impl Harness {
    /// Models the server offers; an empty list when it cannot be asked, not an error, since a server that is
    /// not running yet is normal at startup.
    pub async fn models(&self) -> Vec<crate::protocol::ModelChoice> {
        // Ask the active provider, not an `OllamaAdmin` built at startup, which would keep listing the old server's catalogue.
        let Some(admin) = self.llm.admin() else {
            // Remote providers have no model lifecycle; empty is the right answer, and the UI gets names from the probe.
            return Vec::new();
        };
        match admin.list().await {
            Ok(models) => models
                .into_iter()
                .map(|model| crate::protocol::ModelChoice {
                    id: model.name,
                    tools: model.capabilities.tools,
                    chat: model.capabilities.chat,
                    embedding: model.capabilities.embedding,
                    vision: model.capabilities.vision,
                    context_window: model.capabilities.context_window,
                })
                .collect(),
            Err(err) => {
                tracing::warn!("could not query the model list: {err}");
                Vec::new()
            }
        }
    }

    /// Push the active provider out to everything holding the shared pointer -- sub-agents, model administration
    /// and the document embedder -- after every provider change, so no second path can forget one.
    pub async fn apply_provider(&self) -> Result<(), String> {
        self.providers
            .apply_active()
            .await
            .map_err(|err| err.to_string())?;
        apply_llm(
            &self.providers,
            &self.llm,
            &self.rag_config,
            self.current.lock().as_ref(),
        );
        Ok(())
    }

    /// The user's patch file, which may not exist yet; settings still shows the path so they know where to create it.
    pub fn patch_path(&self) -> PathBuf {
        self.rebuild.config.data_dir.join("patch.yaml")
    }

    pub fn current_project(&self) -> Option<Project> {
        self.current.lock().clone()
    }

    /// The working directory, if a project is open; `None` is not an error and must never be defaulted, since
    /// every caller needs a directory the user actually chose.
    pub fn workspace(&self) -> Option<PathBuf> {
        self.current
            .lock()
            .as_ref()
            .map(|project| PathBuf::from(&project.path))
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> {
        self.projects.list().map_err(|err| err.to_string())
    }

    /// Where a conversation keeps copies of the files attached to it: one folder per session, inside one folder
    /// per project. Per session, so deleting the conversation deletes exactly the copies it caused and two
    /// sessions never overwrite each other's same-named files. Per project, because the folder above is what
    /// `fs` is granted, and a single shared folder would let a turn in one project read what was attached to
    /// another one.
    ///
    /// The id names a directory, so it must be a single path segment; the UI generates it, but a check here is
    /// what keeps a future caller from turning an id into a way out of the data store.
    pub fn session_attachments(&self, session_id: &str) -> Result<PathBuf, String> {
        let mot_doan = !session_id.is_empty()
            && session_id != "."
            && session_id != ".."
            && !session_id.contains(['/', '\\']);
        if !mot_doan {
            return Err(format!("Mã phiên không hợp lệ: {session_id}"));
        }
        let workspace = self
            .workspace()
            .ok_or_else(|| "Chưa mở dự án, nên chưa có nơi để giữ tệp đính kèm.".to_string())?;
        // The canonical form, the same one `fs` was granted: handing the composer a path that resolves
        // elsewhere is how an attachment the app just wrote gets refused by the read tool.
        Ok(attachments_root(&self.rebuild.config.data_dir, &workspace).join(session_id))
    }

    /// Drop a session's attachment copies. Every project folder is checked rather than the open one, since a
    /// session is deleted from wherever the user happens to be, and its id is unique across all of them.
    /// Failing to remove the copies is never worth failing the deletion over, so this only logs.
    pub fn forget_attachments(&self, session_id: &str) {
        if session_id.is_empty() || session_id.contains(['/', '\\']) || session_id.starts_with('.')
        {
            return;
        }
        let goc = attachments_dir(&self.rebuild.config.data_dir);
        let Ok(du_ans) = std::fs::read_dir(&goc) else {
            return;
        };
        for du_an in du_ans.flatten() {
            let dir = du_an.path().join(session_id);
            if !dir.is_dir() {
                continue;
            }
            if let Err(err) = std::fs::remove_dir_all(&dir) {
                tracing::warn!("could not remove {}: {err}", dir.display());
            }
        }
    }

    /// Register a directory as a project with an explicit type, without opening it; separate from
    /// [`Harness::open_project`], which preserves an existing type instead of setting one.
    pub fn create_project(
        &self,
        path: &Path,
        kind: ProjectKind,
        origin: Option<&str>,
    ) -> Result<Project, String> {
        self.projects
            .create(path, kind, origin)
            .map_err(|err| err.to_string())
    }

    /// Change a project's type, reloading the plugin layer immediately when it is the open project, or the
    /// stored row and the running tool set would disagree until the next open.
    pub async fn set_project_kind(&self, id: &str, kind: ProjectKind) -> Result<Project, String> {
        let project = self
            .projects
            .set_kind(id, kind)
            .map_err(|err| err.to_string())?;
        if self
            .current
            .lock()
            .as_ref()
            .is_some_and(|open| open.id == id)
        {
            self.open_project(Path::new(&project.path)).await?;
        }
        Ok(project)
    }

    /// Delete a project: its sessions, its document library, and its row. **The user's folder is never
    /// touched** - the files are theirs, and a library is only an index of them.
    ///
    /// The library goes first. If dropping it fails the project stays, whole, and the user can retry;
    /// forgetting the row first would leave a library nothing can name any more, since the id is the only
    /// handle on it.
    pub async fn delete_project(&self, id: &str) -> Result<(), String> {
        if self
            .current
            .lock()
            .as_ref()
            .is_some_and(|open| open.id == id)
        {
            return Err("hãy chuyển sang dự án khác trước khi xoá dự án đang mở".into());
        }
        let project = self.projects.get(id).map_err(|err| err.to_string())?;

        if project.kind == ProjectKind::Docs {
            self.purge_library(&project).await?;
        }

        // Sessions are addressed by the directory they were opened in, which is exactly what the sidebar
        // lists, so this removes precisely the conversations the user saw under this project.
        let headers = self
            .sessions
            .list(SessionScope::Directory(&project.path), None)
            .await
            .map_err(|err| err.to_string())?;
        for header in headers {
            self.sessions
                .delete(&header.id)
                .await
                .map_err(|err| err.to_string())?;
            self.forget_attachments(&header.id);
        }
        // What the conversations attached goes with them: the extracted text and vectors first, then the
        // folder of copies. Neither failure is worth keeping the project row alive over, so both only log.
        let workspace = Path::new(&project.path);
        if let Err(err) =
            purge_library(self.rag_config.path(), &attachments_project(workspace)).await
        {
            tracing::warn!("could not drop the attachment library: {err}");
        }
        let copies = attachments_dir(&self.rebuild.config.data_dir).join(project_slug(workspace));
        if let Err(err) = std::fs::remove_dir_all(&copies)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("could not remove {}: {err}", copies.display());
        }

        self.projects.forget(id).map_err(|err| err.to_string())
    }

    /// Drop a closed project's native index. User documents live outside it and are never removed.
    async fn purge_library(&self, project: &Project) -> Result<(), String> {
        let slug = project_slug(Path::new(&project.path));
        purge_library(self.rag_config.path(), &slug)
            .await
            .map_err(|err| err.to_string())
    }

    pub fn forget_project(&self, id: &str) -> Result<(), String> {
        if self
            .current
            .lock()
            .as_ref()
            .is_some_and(|open| open.id == id)
        {
            // Dropping the open project would leave the app pointing at something nothing references; switch first, then remove.
            return Err("hãy chuyển sang dự án khác trước khi bỏ dự án đang mở".into());
        }
        self.projects.forget(id).map_err(|err| err.to_string())
    }

    /// Project the provider state and open project into the file native RAG reads; call it wherever
    /// either changes, or the service answers from a stale snapshot with nothing to signal it.
    fn write_rag_config(&self, project: Option<&Project>) {
        match self.providers.list() {
            Ok(rows) => self.rag_config.write(&rows, rag_project(project)),
            Err(err) => tracing::warn!("could not read the provider list: {err}"),
        }
    }

    pub async fn open_project(&self, path: &Path) -> Result<Project, String> {
        let project = self.projects.touch(path).map_err(|err| err.to_string())?;
        // Hold the lock throughout: two overlapping switches would leave half of each project's layer.
        let mut scopes = self.project_scopes.lock().await;

        for scope in scopes.drain(..).rev() {
            scope.effects().dispose().await;
        }

        // Write the RAG config before loading plugins: the service connects lazily, but the first call can
        // arrive at once, and a stale file would serve the previous project's library.
        self.write_rag_config(Some(&project));

        let catalog = catalog(
            &self.rebuild.config,
            Some(Path::new(&project.path)),
            self.rebuild.llm.clone(),
            self.rebuild.rag_config.clone(),
            self.rebuild.sessions.clone(),
            self.rebuild.asr.clone(),
        );
        for row in self
            .rebuild
            .composed
            .active()
            .filter(|row| hop_loai(row, Some(project.kind)))
        {
            let plugin = catalog.build(row).map_err(|err| err.to_string())?;
            let scope = self.rebuild.ctx.plugin(plugin.name());
            plugin.apply(&scope).await.map_err(|err| err.to_string())?;
            scopes.push(scope);
        }

        *self.current.lock() = Some(project.clone());
        tracing::info!(path = %project.path, "switched project");
        Ok(project)
    }

    /// Close the open project without opening a replacement: the same mechanism as switching, minus the second
    /// half. Conversation still runs with no disk-touching tools, which is the app's first-launch state.
    pub async fn close_project(&self) {
        let mut scopes = self.project_scopes.lock().await;
        for scope in scopes.drain(..).rev() {
            scope.effects().dispose().await;
        }
        // Remove the project from the config too, or a live service would still answer about the closed library.
        self.write_rag_config(None);
        *self.current.lock() = None;
        tracing::info!("project closed; conversation runs with no disk-touching tools");
    }

    /// Tear the tree down, children before parents and last-loaded first; process exit cannot send an LSP
    /// `shutdown` or close an MCP session politely. Call it when the window closes.
    pub async fn shutdown(&self) {
        // The project layer goes first: it depends on the application layer's tool registry.
        for scope in self.project_scopes.lock().await.drain(..).rev() {
            scope.effects().dispose().await;
        }
        for scope in self.scopes.iter().rev() {
            scope.effects().dispose().await;
        }
    }
}

pub struct Config {
    pub data_dir: PathBuf,
    /// Skills bundled with the installation; `None` is valid, not a startup error -- the app runs without built-ins.
    pub builtin_skills: Option<PathBuf>,
    /// The project open at startup; `None` means none. It used to default to the current directory, which is
    /// `/` when launched from Finder, giving an unchosen "project" rooted at the disk.
    pub workspace: Option<PathBuf>,
    pub ollama_url: String,
    pub model: String,
    /// Context window, in tokens.
    pub context_window: usize,
    /// The embedding model used to seed the first provider row; after that it is a per-row field edited in the
    /// app, and this variable has no further say.
    pub embed_model: Option<String>,
}

impl Config {
    /// Configuration from the environment, with defaults that work out of the box.
    pub fn from_env() -> Config {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Config {
            builtin_skills: builtin_skills(),
            data_dir: std::env::var("PAI_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(&home).join(".private-ai")),
            // No fallback to the current directory; without this variable [`boot`] reopens the most recent stored project, or none.
            workspace: std::env::var("PAI_WORKSPACE").ok().map(PathBuf::from),
            ollama_url: std::env::var("PAI_OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
            model: std::env::var("PAI_MODEL").unwrap_or_else(|_| "qwen3:8b".into()),
            // Asking the server would be better, but startup must not depend on it running; this fallback errs
            // low, since compacting early costs tokens while compacting late costs the turn.
            embed_model: std::env::var("PAI_EMBED_MODEL").ok(),
            context_window: std::env::var("PAI_CONTEXT_WINDOW")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(32_768),
        }
    }
}

/// A project's own directory name in the data store: folder name plus a hash of the full path, as `pai-index`
/// does, so same-named repos stay separate while the directory remains recognisable.
fn project_slug(workspace: &Path) -> String {
    let name = workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "du-an".to_string());
    // 64-bit FNV-1a, hand-written because this is the only place that hashes and a crate for one path string is a dependency to maintain.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in workspace.display().to_string().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{safe}-{hash:016x}")
}

/// The folder holding every session's attachment copies, under the application data store rather than in the
/// user's project: attaching a file to a conversation must not add a file to their repository.
fn attachments_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("dinh-kem")
}

/// The library id for a project's attachments: its own store and its own vector collection, never the ones a
/// document library at the same path would use.
fn attachments_project(workspace: &Path) -> String {
    format!("{}-dinh-kem", project_slug(workspace))
}

/// The open project's own attachment folder, and the only one `fs` is granted: sessions of other projects keep
/// their copies beside it, where a turn in this project cannot reach them.
///
/// Created and canonicalised first, because [`pai_fs::FileRoots`] compares a canonical path against the root it
/// was given, and on macOS the data store sits behind a symlink often enough that an unresolved root would
/// refuse every attachment.
fn attachments_root(data_dir: &Path, workspace: &Path) -> PathBuf {
    let dir = attachments_dir(data_dir).join(project_slug(workspace));
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!("could not create {}: {err}", dir.display());
    }
    dir.canonicalize().unwrap_or(dir)
}

/// The other half of switching providers, read from the store rather than `Driver`: with nothing configured,
/// `driver.llm()` is [`ActiveLlm`] itself, and pointing it at itself would loop forever in the token path.
fn apply_llm(
    runtime: &ProviderRuntime,
    llm: &ActiveLlm,
    rag_config: &RagConfigFile,
    project: Option<&Project>,
) {
    // Two roles, two independent branches: a chat failure used to return early and leave the embedder unset,
    // yet an unreachable chat server says nothing about the embedding one.
    match runtime.store().active(Role::Chat) {
        Ok(Some(active)) => match runtime.registry().adapter(&active.config) {
            Ok(adapter) => llm.set(adapter),
            Err(err) => tracing::warn!("could not build the chat adapter: {err}"),
        },
        Ok(None) => tracing::warn!("no provider holds the chat role yet"),
        Err(err) => tracing::warn!("could not read the chat provider: {err}"),
    }

    // Rewrite the document-library config. Revoking the embedding role must also be written, or documents keep
    // going to the server the user just detached; an empty model falls back to keyword search, which is correct.
    match runtime.list() {
        Ok(rows) => rag_config.write(&rows, rag_project(project)),
        Err(err) => tracing::warn!("could not read the provider list: {err}"),
    }
}

/// The open project as native RAG needs it -- and `None` for a code project. Source code is
/// indexed by `index` and `lsp`; only document projects are chunked as prose.
fn rag_project(project: Option<&Project>) -> Option<crate::rag_config::Project> {
    let item = project.filter(|item| item.kind == ProjectKind::Docs)?;
    Some(crate::rag_config::Project {
        id: project_slug(Path::new(&item.path)),
        name: item.name.clone(),
        root: PathBuf::from(&item.path),
    })
}

/// The bundled skills directory, probed by path rather than via `AppHandle` because [`boot`] runs before any
/// handle exists: `PAI_SKILLS_DIR`, then the macOS `.app` resources, then next to the executable, then the
/// source tree. `None` when none exist, which is not worth blocking startup over.
fn builtin_skills() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PAI_SKILLS_DIR") {
        let path = PathBuf::from(explicit);
        return path.is_dir().then_some(path);
    }
    let exe = std::env::current_exe().ok()?;
    let near_exe = exe.parent()?;
    let candidates = [
        near_exe.join("../Resources/skills"),
        near_exe.join("skills"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills"),
    ];
    candidates.into_iter().find(|path| path.is_dir())
}

/// Optional environment file shared with the root Docker Compose stack.
pub(crate) fn rag_env_file() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PAI_RAG_ENV_FILE") {
        return Some(PathBuf::from(explicit));
    }
    let from_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    let candidates = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(".env")))
        .into_iter()
        .chain(std::iter::once(from_source));
    candidates.into_iter().find(|path| path.is_file())
}

/// The default plugin tree, written in the same format users patch; kept in code rather than beside the
/// installation, where an edited file would be lost at the first update.
const BASE: &str = r#"
patches:
  - op: insert
    id: tools
    plugin: tools
  - op: insert
    id: agent
    plugin: agent
  - op: insert
    id: compaction
    plugin: compaction
  - op: insert
    id: hooks
    plugin: hooks
    config:
      hooks: []
  - op: insert
    id: sandbox
    plugin: sandbox
  - op: insert
    id: mcp
    plugin: mcp
    config:
      servers: []
      expose:
        stdio: false
  - op: insert
    id: skills
    plugin: skills
  - op: insert
    id: fs
    plugin: fs
  - op: insert
    id: subagent
    plugin: subagent
  - op: insert
    id: rag
    plugin: rag
  - op: insert
    id: attachments
    plugin: attachments
  - op: insert
    id: index
    plugin: index
  - op: insert
    id: lsp
    plugin: lsp
  - op: insert
    id: shell
    plugin: shell
  - op: insert
    id: terminal
    plugin: terminal
"#;

/// Code projects: read, edit, run, look up. Project-layer plugins each capture a path when built, so switching
/// projects means rebuilding them; the layer is chosen by plugin name, and there is no second reconfigure path.
const CODE_PLUGINS: &[&str] = &[
    "skills",
    "fs",
    "attachments",
    "subagent",
    "index",
    "lsp",
    "shell",
    "terminal",
];

/// Document projects: search and read only. Every omission is a decision -- a library is files other people
/// sent, so `shell` and `edit` would grant execution and overwrite where content is least trusted, and
/// `index` and `lsp` analyse source code that is not here.
const DOCS_PLUGINS: &[&str] = &["skills", "rag", "subagent"];

/// Project-layer plugins, rebuilt on every switch because each captures a path when built.
fn thuoc_du_an(row: &Row) -> bool {
    CODE_PLUGINS.contains(&row.plugin.as_str()) || DOCS_PLUGINS.contains(&row.plugin.as_str())
}

/// Project-layer plugins that also suit the open project's type -- where the type actually takes effect.
/// Unsuitable tools are never loaded, so there is nothing to disable and no parallel path to drift.
fn hop_loai(row: &Row, kind: Option<ProjectKind>) -> bool {
    match kind {
        Some(ProjectKind::Code) => CODE_PLUGINS,
        Some(ProjectKind::Docs) => DOCS_PLUGINS,
        // With no project, no project-layer plugin loads: each captures a path and there is none to capture.
        // Conversation still runs, which is the whole point of this state.
        None => &[],
    }
    .contains(&row.plugin.as_str())
}

/// Last line of defence for project-layer plugins with no project; [`hop_loai`] already filtered them out, so
/// this exists to fail loudly rather than hand `fs` and `shell` a fabricated root.
fn khong_co_du_an() -> anyhow::Error {
    anyhow::anyhow!("plugin này cần một dự án đang mở, nhưng chưa có dự án nào")
}

/// The plugin builder registry, bound to one project; `workspace` is explicit rather than read from `config`,
/// or the second build would silently reuse the startup path.
fn catalog(
    config: &Config,
    workspace: Option<&Path>,
    llm: Arc<ActiveLlm>,
    rag_config: Arc<RagConfigFile>,
    sessions: SessionService,
    asr: Asr,
) -> PluginCatalog {
    let mut catalog = PluginCatalog::new();
    let identity = IDENTITY.to_string();
    // Project-layer plugins are built only with a project; assert it again with a readable error rather than a fabricated path.
    let workspace = workspace.map(Path::to_path_buf);
    let data_dir = config.data_dir.clone();
    let window = config.context_window;

    catalog.register("tools", |_| Ok(Box::new(ToolsPlugin) as Box<dyn Plugin>));
    catalog.register("agent", move |_| {
        Ok(Box::new(AgentPlugin::new(identity.clone())) as Box<dyn Plugin>)
    });
    catalog.register("compaction", move |_| {
        Ok(Box::new(CompactionPlugin::new(window)) as Box<dyn Plugin>)
    });
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        let builtin = config.builtin_skills.clone();
        catalog.register("skills", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // Three sources scanned in order, each replacing same-named bundles from the previous: built-ins,
            // the user's data store, then the repo's own. The order is an authority ladder.
            let mut roots = Vec::with_capacity(3);
            roots.extend(builtin.clone());
            roots.push(data_dir.join("skills"));
            roots.push(workspace.join(".pai/skills"));
            Ok(Box::new(SkillsPlugin::new(roots)) as Box<dyn Plugin>)
        });
    }
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        catalog.register("fs", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // The workspace, plus the one folder inside the data store that holds files the user attached to a
            // conversation -- a file attached from outside the project is copied there, and a copy nothing may
            // read is not an attachment. The rest of the data store stays out, since a convenience grant is how
            // a settings file gets edited by a sentence inside a freshly ingested document. The token path comes
            // from `pai-mcp` itself so two crates cannot drift apart on it.
            Ok(Box::new(FsPlugin::new(
                [workspace.clone(), attachments_root(&data_dir, &workspace)],
                [token_path(&data_dir)],
            )) as Box<dyn Plugin>)
        });
    }
    catalog.register("hooks", |value| {
        let row: HooksRow = serde_json::from_value(value.clone())?;
        Ok(Box::new(HooksPlugin::new(row.hooks)) as Box<dyn Plugin>)
    });
    {
        let (workspace, model) = (workspace.clone(), config.model.clone());
        catalog.register("subagent", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // The shared pointer, not an adapter copy: a sub-agent must reach the same provider as its parent turn.
            let llm: Arc<dyn LlmAdapter> = llm.clone();
            Ok(Box::new(SubagentPlugin::new(
                llm,
                sessions.clone(),
                model.clone(),
                workspace.display().to_string(),
            )) as Box<dyn Plugin>)
        });
    }
    {
        let workspace = workspace.clone();
        let rag_config = rag_config.clone();
        let asr = asr.clone();
        catalog.register("rag", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // The chosen directory is the library, read directly with no copy to drift from disk.
            // The project id travels with every call and names the Qdrant collection, so it uses `project_slug`
            // rather than the folder name.
            Ok(Box::new(RagPlugin::new(
                rag_config.path().to_path_buf(),
                project_slug(&workspace),
                workspace.clone(),
                asr.clone(),
            )) as Box<dyn Plugin>)
        });
    }
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        let rag_config = rag_config.clone();
        let asr = asr.clone();
        catalog.register("attachments", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // The same library a document project runs, mounted over the folder holding what was attached to
            // this project's conversations. That is what makes an attached PDF, image or DOCX readable at all:
            // `read` refuses bytes, while this side already has pdfium, the DOCX reader and OCR through the
            // vision role. A distinct project id, so its store and vector collection never collide with the
            // document library of a project at the same path.
            Ok(Box::new(RagPlugin::attachments(
                rag_config.path().to_path_buf(),
                attachments_project(&workspace),
                attachments_root(&data_dir, &workspace),
                asr.clone(),
            )) as Box<dyn Plugin>)
        });
    }
    catalog.register("sandbox", |_| {
        Ok(Box::new(SandboxPlugin::new()) as Box<dyn Plugin>)
    });
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        catalog.register("index", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // The same roots and guards as `fs`: an index that sees more than the read tool is a way around that boundary.
            Ok(Box::new(IndexPlugin::new(
                [workspace.clone()],
                [token_path(&data_dir)],
                data_dir.join("index"),
            )) as Box<dyn Plugin>)
        });
    }
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        catalog.register("lsp", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            // Same roots and guards as `fs` and `index`; with no language server installed the plugin loads and registers no tools, which is valid.
            Ok(Box::new(LspPlugin::new(
                [workspace.clone()],
                [token_path(&data_dir)],
                workspace.clone(),
            )) as Box<dyn Plugin>)
        });
    }
    {
        let workspace = workspace.clone();
        catalog.register("terminal", move |_| {
            let Some(workspace) = workspace.clone() else {
                return Err(khong_co_du_an());
            };
            Ok(Box::new(TerminalPlugin::new(workspace.clone())) as Box<dyn Plugin>)
        });
    }
    catalog.register("shell", move |_| {
        let Some(workspace) = workspace.clone() else {
            return Err(khong_co_du_an());
        };
        Ok(Box::new(ShellPlugin::new(workspace.clone())) as Box<dyn Plugin>)
    });
    {
        let data_dir = data_dir.clone();
        catalog.register("mcp", move |value| {
            let row: McpRow = serde_json::from_value(value.clone())?;
            let mut plugin = McpPlugin::new(row.servers).storing(data_dir.join("mcp.json"));
            // Exposure is off by default: opening a port, even on loopback, is outward-facing and must be switched on deliberately.
            if row.expose.stdio || row.expose.http.is_some() {
                plugin = plugin.exposing(ExposeOptions {
                    data_dir: data_dir.clone(),
                    stdio: row.expose.stdio,
                    http: row.expose.http,
                    allowed_origins: row.expose.allowed_origins,
                });
            }
            Ok(Box::new(plugin) as Box<dyn Plugin>)
        });
    }
    catalog
}

/// Configuration of the `hooks` row.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct HooksRow {
    hooks: Vec<HookConfig>,
}

/// Configuration of the `mcp` row, declared here rather than in `pai-mcp` because a config row's shape belongs to whoever builds the tree.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct McpRow {
    servers: Vec<ServerConfig>,
    expose: ExposeRow,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ExposeRow {
    stdio: bool,
    http: Option<std::net::SocketAddr>,
    allowed_origins: Vec<String>,
}

/// The configuration layers in application order, base then user; layering happens once for the whole tree and
/// the split into tiers comes after, so a patch need not know about tiers.
fn layers(config: &Config) -> anyhow::Result<Vec<Layer>> {
    let base: Layer = serde_norway::from_str(BASE)?;
    let mut layers = vec![Layer {
        origin: "nen (dựng sẵn)".into(),
        ..base
    }];

    let path = config.data_dir.join("patch.yaml");
    if path.is_file() {
        let text = std::fs::read_to_string(&path)?;
        let user: Layer = serde_norway::from_str(&text)
            // A broken patch file stops startup rather than being skipped: continuing on defaults looks exactly like working.
            .map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))?;
        layers.push(Layer {
            origin: path.display().to_string(),
            ..user
        });
    }
    Ok(layers)
}

pub async fn boot(config: Config) -> anyhow::Result<Harness> {
    std::fs::create_dir_all(&config.data_dir)?;
    let ctx = Context::root();

    // The session store and model adapter are built before the plugin loop, because `subagent` needs both:
    // a sub-agent is a complete turn. They are not plugins; they are what plugins use.
    let store: Arc<dyn SessionStore> =
        Arc::new(SqliteSessionStore::open(config.data_dir.join("phien.db"))?);
    let sessions = SessionService::new(store);
    let http = reqwest::Client::new();

    // The provider store, seeded once if empty: environment configuration becomes the first stored row rather
    // than a parallel path, so there is one source of truth the user can edit in the app.
    let providers: Arc<dyn ProviderStore> =
        Arc::new(SqliteProviderStore::open(config.data_dir.join(DB_FILE))?);
    if providers.list()?.is_empty() {
        let seeded = providers.save(
            ProviderInput::create(
                "Ollama trên máy này",
                ProviderKind::Ollama,
                config.ollama_url.clone(),
            )
            .with_model(config.model.clone()),
        )?;
        providers.activate(Role::Chat, seeded.id(), Some(&config.model))?;
        // The seeded row is a local Ollama, so granting it the embedding role sends nothing outward and is the
        // right default: documents get ingested before anyone opens settings.
        let embed_model = config
            .embed_model
            .clone()
            .unwrap_or_else(|| pai_providers::DEFAULT_EMBEDDING_MODEL_OLLAMA.to_string());
        providers.activate(Role::Embedding, seeded.id(), Some(&embed_model))?;
    }

    // The active-provider pointer, built before the plugin loop because `subagent` needs it at load time.
    let boot_adapter: Arc<dyn LlmAdapter> = Arc::new(OllamaAdapter::new(
        "ollama",
        &config.ollama_url,
        http.clone(),
    ));
    let llm = Arc::new(ActiveLlm::new(boot_adapter));
    let rag_env = rag_env_file();
    let rag_config = Arc::new(RagConfigFile::new(&config.data_dir, rag_env.as_deref()));

    let projects: Arc<dyn ProjectStore> =
        Arc::new(SqliteProjectStore::open(config.data_dir.join("du-an.db"))?);
    // The startup project is an ordinary project that happens to be open, chosen in three tiers: environment
    // variable, most recent stored project, then none -- which is the first-launch state, not a gap.
    let project = match &config.workspace {
        Some(path) => Some(projects.touch(path)?),
        None => match projects.list()?.into_iter().next() {
            // `list` returns newest first, so the first entry is the most recent; `touch` refreshes its time and
            // turns a deleted directory into a skipped project rather than a failed startup.
            Some(last) => match projects.touch(Path::new(&last.path)) {
                Ok(project) => Some(project),
                Err(err) => {
                    tracing::warn!(path = %last.path, "could not reopen the most recent project: {err}");
                    None
                }
            },
            None => None,
        },
    };
    let kind = project.as_ref().map(|open| open.kind);

    let composed = compose(&layers(&config)?)?;
    // Extract the `mcp` row's server list once, while the config tree is still intact.
    let mcp_rows = composed
        .active()
        .find(|row| row.plugin == "mcp")
        .and_then(|row| serde_json::from_value::<McpRow>(row.config.clone()).ok())
        .map(|row| row.servers)
        .unwrap_or_default();
    tracing::debug!("plugin tree:\n{}", composed.dump());
    // The RAG configuration file is read by every library as it loads -- the document library, and the
    // attachment library every code project now carries. The provider pass that normally writes it runs after
    // the plugins, so write it once here, or a first launch fails on a file nobody has produced yet.
    match providers.list() {
        Ok(rows) => rag_config.write(&rows, rag_project(project.as_ref())),
        Err(err) => tracing::warn!("could not read the provider list: {err}"),
    }

    // Built before the plugin tree: both library mounts take a clone, and dictation needs one even
    // with no project open. A first launch has no model chosen; if one is already sitting in the data
    // directory, seed it and write that choice down, so the settings screen shows the same model the
    // library is about to use rather than an empty field over a working feature.
    let mut asr_config = rag_config.asr();
    if asr_config.model_path().is_none()
        && let Some(found) = pai_asr::discover_model(&config.data_dir)
    {
        asr_config.model = found;
        if let Err(err) = rag_config.write_asr(&asr_config) {
            tracing::warn!("could not seed the speech model setting: {err}");
        }
    }
    let asr = Asr::new(asr_config);
    let catalog = catalog(
        &config,
        project.as_ref().map(|open| Path::new(open.path.as_str())),
        llm.clone(),
        rag_config.clone(),
        sessions.clone(),
        asr.clone(),
    );

    // List order does not decide load order: dependencies are expressed with `require`. Keep the scopes rather
    // than forgetting them, or no plugin's async teardown ever runs and orphan processes survive the app.
    let mut scopes = Vec::new();
    let mut project_scopes = Vec::new();
    for row in composed.active() {
        // Skip before building: a `rag` row in a code project has nothing to build, and building then discarding opens a database nobody reads.
        if thuoc_du_an(row) && !hop_loai(row, kind) {
            continue;
        }
        let plugin = catalog.build(row)?;
        let scope = ctx.plugin(plugin.name());
        plugin.apply(&scope).await?;
        if thuoc_du_an(row) {
            project_scopes.push(scope);
        } else {
            scopes.push(scope);
        }
    }

    let registry: Arc<ToolRegistry> = ctx.require::<Tools>()?;
    let pipeline = Arc::new(ToolPipeline::new(&ctx, registry));
    let prompt: Arc<SystemPrompt> = ctx.require::<Prompt>()?;

    let driver = Arc::new(Driver::new(
        ctx.clone(),
        llm.clone() as Arc<dyn LlmAdapter>,
        pipeline,
        prompt,
        config.model.clone(),
    ));

    // The provider layer is built after `Driver` because it pushes adapters into it; that is why it is not a
    // plugin row, since plugins load before `Driver` exists.
    let runtime = Arc::new(ProviderRuntime::new(
        providers.clone(),
        Arc::new(AdapterRegistry::new(http.clone())),
        driver.clone(),
        http.clone(),
    ));
    // Hand the guard to the root scope instead of keeping it in `Harness`: `Guard` is not `Sync` while `Harness`
    // lives in Tauri `State` and must be. The root scope lasts the process, so only the storage location changes.
    ctx.keep(ctx.provide::<Providers>(runtime.clone())?);
    // An unconfigured machine is normal on a fresh install, not a startup error: the provider screen is where it gets fixed.
    if let Err(err) = runtime.apply_active().await {
        tracing::warn!("no provider is usable yet: {err}");
    }
    // Pass the restored project, not `None`: `boot` builds the recent project's layer here rather than through
    // `open_project`, so this is the only place its config gets written.
    apply_llm(&runtime, &llm, &rag_config, project.as_ref());
    Ok(Harness {
        ctx: ctx.clone(),
        context_window: config.context_window,
        sessions: sessions.clone(),
        driver,
        plugins: composed.clone(),
        scopes,
        project_scopes: tokio::sync::Mutex::new(project_scopes),
        projects,
        current: parking_lot::Mutex::new(project),
        providers: runtime,
        mcp_rows,
        rebuild: Rebuild {
            ctx,
            config,
            llm: llm.clone(),
            rag_config: rag_config.clone(),
            asr: asr.clone(),
            sessions,
            composed,
        },
        llm,
        rag_config,
        asr,
    })
}
