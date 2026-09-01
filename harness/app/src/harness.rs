//! Dựng cây plugin.
//!
//! Toàn bộ việc "ứng dụng này gồm những gì" nằm trong đúng một hàm, và nó đọc như một
//! danh sách. Đó là điểm của kiến trúc plugin: thêm một khả năng là thêm một dòng ở đây,
//! bớt đi là xoá một dòng, và không có chỗ nào khác phải biết.
//!
//! Về sau danh sách này đến từ tệp cấu hình theo lớp (profile/bundle/patch). Cho tới lúc
//! đó nó nằm trong mã, vì một trình nạp cấu hình chưa ai dùng là một trình nạp chưa ai
//! kiểm chứng.

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
    DB_FILE, ProviderInput, ProviderRuntime, ProviderStore, Providers, SqliteProviderStore,
};
use pai_rag::{Embedder, OllamaEmbedder, OpenAiEmbedder, RagPlugin};
use pai_sandbox::SandboxPlugin;
use pai_session::{SessionService, SessionStore, SqliteSessionStore};
use pai_shell::ShellPlugin;
use pai_terminal::TerminalPlugin;
use pai_tools::{ToolPipeline, ToolRegistry, Tools, ToolsPlugin};

use crate::llm::{ActiveEmbedder, ActiveLlm};

/// Lời tự giới thiệu đứng đầu mọi prompt.
const IDENTITY: &str = "\
Bạn là trợ lý lập trình chạy trên máy của người dùng. Bạn đọc và sửa mã nguồn trong thư \
mục làm việc, chạy lệnh khi cần, và nói tiếng Việt.

Trước khi sửa một tệp, hãy đọc nó. Trước khi kết luận, hãy kiểm chứng. Khi một việc \
không làm được, hãy nói ra thay vì làm một việc gần giống.";

pub struct Harness {
    pub ctx: Context,
    pub sessions: SessionService,
    pub driver: Arc<Driver>,
    /// Cây đã áp lớp, giữ lại để trả lời câu hỏi "bản đang chạy gồm những gì".
    pub plugins: Composed,
    /// Scope của plugin ứng dụng, theo đúng thứ tự đã cắm. Sống bằng tiến trình.
    scopes: Vec<Context>,
    /// Scope của plugin thuộc dự án. Bị tháo và dựng lại mỗi lần đổi dự án.
    project_scopes: tokio::sync::Mutex<Vec<Context>>,
    projects: Arc<dyn ProjectStore>,
    current: parking_lot::Mutex<Project>,
    /// Con trỏ tới provider đang hoạt động. Mọi chỗ cần nói chuyện với mô hình đều cầm
    /// **cái này**, không cầm một bản sao — xem `crate::llm`.
    pub llm: Arc<ActiveLlm>,
    pub embedder: Arc<ActiveEmbedder>,
    pub providers: Arc<ProviderRuntime>,
    /// Server MCP khai trong **hàng cấu hình** (`patch.yaml`), giữ nguyên để mỗi lần nạp
    /// lại còn truyền vào được.
    ///
    /// `pai_mcp::apply` gỡ mọi server không nằm trong danh sách nó nhận. Truyền một danh
    /// sách rỗng nghĩa là server người dùng khai trong bản vá biến mất ngay lần nạp lại
    /// đầu tiên — im lặng, và họ sẽ đi tìm xem bản vá của mình hỏng ở đâu.
    pub mcp_rows: Vec<ServerConfig>,
    /// Đủ để dựng lại tầng dự án. Giữ nguyên `Config` thì tiện hơn, nhưng nó mang cả
    /// `workspace` — và một trường nói "thư mục làm việc" mà không còn đúng sau lần đổi
    /// dự án đầu tiên là một cái bẫy đặt sẵn.
    rebuild: Rebuild,
}

/// Những gì `open_project` cần để dựng lại tầng dự án.
struct Rebuild {
    ctx: Context,
    config: Config,
    llm: Arc<ActiveLlm>,
    embedder: Arc<ActiveEmbedder>,
    sessions: SessionService,
    composed: Composed,
}

impl Harness {
    /// Mô hình máy chủ đang có.
    ///
    /// Trả danh sách rỗng khi không hỏi được, không trả lỗi: một máy chủ chưa bật là
    /// trạng thái bình thường lúc khởi động, và một hộp thoại lỗi ở đó chỉ dạy người dùng
    /// bấm cho qua.
    pub async fn models(&self) -> Vec<crate::protocol::ModelChoice> {
        // Hỏi provider **đang hoạt động**, không hỏi một `OllamaAdmin` dựng lúc khởi động.
        // Bản dựng lúc khởi động sẽ vẫn liệt kê kho của máy chủ cũ sau khi người dùng đổi
        // sang một provider khác — im lặng, và trông y hệt như đúng.
        let Some(admin) = self.llm.admin() else {
            // Provider từ xa không có nửa vòng đời mô hình. Danh sách rỗng là câu trả lời
            // đúng ở đây; giao diện lấy tên mô hình từ lần thử kết nối.
            return Vec::new();
        };
        match admin.list().await {
            Ok(models) => models
                .into_iter()
                .map(|model| crate::protocol::ModelChoice {
                    id: model.name,
                    tools: model.capabilities.tools,
                    context_window: model.capabilities.context_window,
                })
                .collect(),
            Err(err) => {
                tracing::warn!("không hỏi được danh sách mô hình: {err}");
                Vec::new()
            }
        }
    }

    /// Đẩy provider đang hoạt động ra mọi chỗ cầm con trỏ chia sẻ.
    ///
    /// `ProviderRuntime` tự lo phần `Driver`; đây là nửa còn lại — agent con, phần quản
    /// trị mô hình, và bộ nhúng của thư viện tài liệu. Gọi nó sau **mọi** thay đổi
    /// provider, và chỉ gọi nó, để không có đường thứ hai nào quên mất một chỗ.
    pub async fn apply_provider(&self) -> Result<(), String> {
        self.providers
            .apply_active()
            .await
            .map_err(|err| err.to_string())?;
        apply_llm(
            &self.providers,
            &self.llm,
            &self.embedder,
            &self.rebuild.config,
        );
        Ok(())
    }

    pub fn current_project(&self) -> Project {
        self.current.lock().clone()
    }

    pub fn workspace(&self) -> PathBuf {
        PathBuf::from(self.current.lock().path.clone())
    }

    pub fn projects(&self) -> Result<Vec<Project>, String> {
        self.projects.list().map_err(|err| err.to_string())
    }

    /// Ghi nhận một thư mục thành dự án với loại tường minh. **Không mở nó.**
    ///
    /// Tách khỏi [`Harness::open_project`] vì hai việc có hai ngữ nghĩa khác nhau về loại:
    /// mở thì giữ nguyên loại đã có, tạo thì đặt loại. Gộp lại thành một hàm có tham số
    /// `Option<ProjectKind>` sẽ đúng ở cả hai chỗ gọi, và sai ở chỗ gọi thứ ba.
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

    pub fn forget_project(&self, id: &str) -> Result<(), String> {
        if self.current.lock().id == id {
            // Bỏ dự án đang mở khỏi danh sách sẽ để ứng dụng trỏ vào một chỗ không còn ai
            // nhắc tới. Chuyển sang dự án khác trước, rồi mới bỏ.
            return Err("hãy chuyển sang dự án khác trước khi bỏ dự án đang mở".into());
        }
        self.projects.forget(id).map_err(|err| err.to_string())
    }

    /// Đổi dự án: tháo tầng plugin thuộc dự án, rồi cắm lại với đường dẫn mới.
    ///
    /// Đây là toàn bộ cơ chế. Không có bước "cập nhật gốc của fs", "đổi cwd của shell",
    /// "trỏ chỉ mục sang chỗ khác" — mỗi bước như thế là một chỗ để quên, và cái quên đó
    /// chỉ lộ ra khi một tool đọc nhầm repo. Cắm lại thì không quên được: plugin nào cũng
    /// đi qua đúng một đường khởi tạo, đường mà nó đã đi lúc khởi động.
    pub async fn open_project(&self, path: &Path) -> Result<Project, String> {
        let project = self.projects.touch(path).map_err(|err| err.to_string())?;
        // Giữ khoá suốt cả quá trình: hai lần đổi dự án chồng lên nhau sẽ để lại một nửa
        // tầng của dự án này và một nửa của dự án kia.
        let mut scopes = self.project_scopes.lock().await;

        for scope in scopes.drain(..).rev() {
            scope.effects().dispose().await;
        }

        let catalog = catalog(
            &self.rebuild.config,
            Path::new(&project.path),
            self.rebuild.llm.clone(),
            self.rebuild.embedder.clone(),
            self.rebuild.sessions.clone(),
        );
        for row in self
            .rebuild
            .composed
            .active()
            .filter(|row| hop_loai(row, project.kind))
        {
            let plugin = catalog.build(row).map_err(|err| err.to_string())?;
            let scope = self.rebuild.ctx.plugin(plugin.name());
            plugin.apply(&scope).await.map_err(|err| err.to_string())?;
            scopes.push(scope);
        }

        *self.current.lock() = project.clone();
        tracing::info!(path = %project.path, "đã đổi dự án");
        Ok(project)
    }

    /// Tháo cây, con trước cha, plugin cắm sau tháo trước.
    ///
    /// Thoát tiến trình dọn được phần lớn thứ này, nhưng không dọn được thứ cần nói lời
    /// tạm biệt: một `shutdown` gửi cho language server, một phiên MCP đóng tử tế. Gọi nó
    /// khi cửa sổ đóng.
    pub async fn shutdown(&self) {
        // Tầng dự án tháo trước: nó phụ thuộc vào sổ đăng ký tool của tầng ứng dụng, nên
        // tháo ngược lại là gỡ cái nền ra khỏi dưới chân thứ đang đứng trên nó.
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
    /// Bộ skill đi kèm bản cài đặt. `None` khi không tìm thấy — và đó là trạng thái hợp
    /// lệ, không phải lỗi khởi động: ứng dụng vẫn chạy, chỉ là không có skill dựng sẵn.
    pub builtin_skills: Option<PathBuf>,
    pub workspace: PathBuf,
    pub ollama_url: String,
    pub model: String,
    /// Cửa sổ ngữ cảnh, tính bằng token.
    pub context_window: usize,
    /// Mô hình nhúng cho thư viện tài liệu. `None` = để `build_embedder` chọn theo loại
    /// nhà cung cấp, vì tên mặc định của mỗi bên một khác.
    pub embed_model: Option<String>,
}

impl Config {
    /// Cấu hình từ môi trường, với mặc định dùng được ngay.
    pub fn from_env() -> Config {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Config {
            builtin_skills: builtin_skills(),
            data_dir: std::env::var("PAI_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from(&home).join(".private-ai")),
            workspace: std::env::var("PAI_WORKSPACE")
                .map(PathBuf::from)
                .or_else(|_| std::env::current_dir())
                .unwrap_or_else(|_| PathBuf::from(&home)),
            ollama_url: std::env::var("PAI_OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
            model: std::env::var("PAI_MODEL").unwrap_or_else(|_| "qwen3:8b".into()),
            // Hỏi máy chủ được thì tốt hơn, nhưng khởi động không nên phụ thuộc vào việc
            // máy chủ có đang chạy hay không. Con số này là chỗ lùi về, và nó thấp hơn
            // thực tế — nén sớm hơn cần thiết thì mất token, nén muộn thì mất cả lượt.
            embed_model: std::env::var("PAI_EMBED_MODEL").ok(),
            context_window: std::env::var("PAI_CONTEXT_WINDOW")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(32_768),
        }
    }
}

/// Tên thư mục riêng cho một dự án trong kho dữ liệu.
///
/// Tên thư mục cộng một băm của đường dẫn đầy đủ, theo đúng lối `pai-index` đã dùng: chỉ
/// tên thì hai repo cùng tên `harness` ở hai chỗ sẽ dùng chung một thư viện, còn chỉ băm
/// thì không ai nhìn vào thư mục dữ liệu mà đoán được nó thuộc dự án nào.
fn project_slug(workspace: &Path) -> String {
    let name = workspace
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "du-an".to_string());
    // FNV-1a 64 bit, viết tay vì đây là chỗ duy nhất cần băm và một crate băm cho một
    // chuỗi đường dẫn là một phụ thuộc phải nuôi qua từng bản phát hành.
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

/// Nửa còn lại của việc đổi provider.
///
/// Đọc từ **kho**, không đọc từ `Driver`. Đọc từ `Driver` thì lúc chưa cấu hình được
/// provider nào, `driver.llm()` vẫn đang là chính [`ActiveLlm`] — và đặt nó làm cái mà
/// `ActiveLlm` trỏ tới là dựng một vòng lặp vô hạn ngay trong đường gửi token. Kho thì
/// không bao giờ trả về chính nó.
fn apply_llm(
    runtime: &ProviderRuntime,
    llm: &ActiveLlm,
    embedder: &ActiveEmbedder,
    config: &Config,
) {
    let active = match runtime.store().active() {
        Ok(Some(active)) => active,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!("không đọc được nhà cung cấp đang chọn: {err}");
            return;
        }
    };
    match runtime.registry().adapter(&active.config) {
        Ok(adapter) => llm.set(adapter),
        Err(err) => {
            tracing::warn!("không dựng được adapter: {err}");
            return;
        }
    }
    embedder.set(Some(build_embedder(&active.config, config)));
}

/// Bộ nhúng cho một provider.
///
/// Mô hình nhúng **khác** mô hình hội thoại, và tên mặc định của nó khác nhau theo từng
/// nhà cung cấp — nên nó không dùng chung `config.model`. `PAI_EMBED_MODEL` đè lên cả hai
/// nhánh, vì người đặt biến đó biết rõ họ đang muốn gì.
fn build_embedder(provider: &pai_llm::ProviderConfig, config: &Config) -> Arc<dyn Embedder> {
    match provider.kind {
        ProviderKind::Ollama => {
            let model = config
                .embed_model
                .clone()
                .unwrap_or_else(|| "nomic-embed-text".to_string());
            Arc::new(OllamaEmbedder::new(&provider.base_url, model))
        }
        ProviderKind::OpenAiCompatible => {
            let model = config
                .embed_model
                .clone()
                .unwrap_or_else(|| "text-embedding-3-small".to_string());
            Arc::new(OpenAiEmbedder::new(
                &provider.base_url,
                model,
                provider.api_key.clone(),
            ))
        }
    }
}

/// Thư mục skill đi kèm bản cài đặt.
///
/// Dò theo đường dẫn thay vì hỏi `AppHandle`, vì [`boot`] chạy trước khi có handle nào và
/// đổi chữ ký của nó chỉ để lấy một đường dẫn là kéo cả Tauri vào một hàm vốn không biết
/// gì về Tauri. Ba chỗ, theo đúng thứ tự đáng tin:
///
/// 1. `PAI_SKILLS_DIR` — lối thoát cho người phát triển và cho bộ test.
/// 2. `…/Contents/Resources/skills` — chỗ Tauri đặt tài nguyên trong bản `.app` của macOS.
/// 3. `…/skills` cạnh chính tệp thực thi — chỗ nó nằm khi chạy `tauri dev` và trên Linux.
/// 4. `<mã nguồn>/skills` — chỉ khi chạy từ cây mã nguồn.
///
/// Không có cái nào thì trả `None`. Một danh sách skill rỗng là một danh sách skill rỗng;
/// nó không đáng để chặn khởi động.
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

/// Cây plugin mặc định, viết bằng chính định dạng mà người dùng vá.
///
/// Nằm trong mã chứ không trong một tệp cạnh bản cài đặt, vì một tệp cạnh bản cài đặt là
/// một tệp người dùng sẽ sửa rồi mất sau lần cập nhật đầu tiên. Muốn đổi thì vá ở lớp
/// trên — và lớp trên thì không bao giờ bị ghi đè.
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

/// Plugin **thuộc về một dự án**: tháo ra và cắm lại mỗi lần đổi dự án.
///
/// Mỗi cái trong đây bắt lấy một đường dẫn lúc dựng — gốc được phép đọc/ghi, thư mục làm
/// việc của shell, gốc chỉ mục, gốc LSP. Đổi dự án nghĩa là những giá trị đó đổi, và cách
/// duy nhất đúng để đổi chúng là dựng lại.
///
/// Phân tầng theo **tên plugin**, không theo tệp cấu hình hay theo `id`. Theo tệp thì bản
/// vá của người dùng phải biết chuyện chia tầng; theo `id` thì đổi tên một hàng là lặng lẽ
/// đổi tầng của nó. Tên plugin là thứ duy nhất nói đúng bản chất: plugin này có cần một
/// đường dẫn không.
///
/// Đây là chỗ kiến trúc plugin phải trả nợ. Nếu đổi dự án cần một đường "cấu hình lại mọi
/// thứ" chạy song song với đường cắm plugin, hai đường đó sẽ trôi ra khỏi nhau và đường
/// thứ hai sẽ luôn thiếu một thứ. Ở đây không có đường thứ hai: [`Harness::open_project`]
/// gọi đúng `dispose()` rồi đúng `apply()`.
/// Dự án mã nguồn: đọc, sửa, chạy, tra.
const CODE_PLUGINS: &[&str] = &[
    "skills", "fs", "subagent", "index", "lsp", "shell", "terminal",
];

/// Dự án tài liệu: **chỉ tìm và đọc**.
///
/// Danh sách này ngắn hơn hẳn, và mỗi cái vắng mặt là một quyết định chứ không phải một
/// chỗ chưa làm. Một thư viện tài liệu là một chồng tệp do người khác gửi tới; cấp cho nó
/// `shell` hay `edit` là mở đường thi hành lệnh và ghi đè tệp ở đúng nơi nội dung không
/// đáng tin nhất đang nằm. `index` và `lsp` cũng không có mặt: chúng phân tích mã nguồn,
/// và ở đây không có mã nguồn.
const DOCS_PLUGINS: &[&str] = &["skills", "rag", "subagent"];

/// Plugin thuộc tầng dự án — tháo ra và cắm lại mỗi lần đổi dự án.
///
/// Mỗi cái trong đây bắt lấy một đường dẫn lúc dựng. Đổi dự án nghĩa là những giá trị đó
/// đổi, và cách duy nhất đúng để đổi chúng là dựng lại.
fn thuoc_du_an(row: &Row) -> bool {
    CODE_PLUGINS.contains(&row.plugin.as_str()) || DOCS_PLUGINS.contains(&row.plugin.as_str())
}

/// Plugin thuộc tầng dự án **và** hợp với loại của dự án đang mở.
///
/// Đây là chỗ loại dự án thật sự có hiệu lực. Không có bước "tắt tool cho dự án tài liệu"
/// nào chạy song song — một bước như thế sẽ trôi ra khỏi danh sách này và sớm muộn để sót
/// một tool. Tool không hợp thì **không được cắm ngay từ đầu**, nên không có gì để tắt.
fn hop_loai(row: &Row, kind: ProjectKind) -> bool {
    match kind {
        ProjectKind::Code => CODE_PLUGINS,
        ProjectKind::Docs => DOCS_PLUGINS,
    }
    .contains(&row.plugin.as_str())
}

/// Sổ dựng plugin, gắn với **một** dự án.
///
/// Nhận `workspace` tường minh chứ không đọc từ `config`: tầng dự án được dựng lại mỗi
/// lần đổi dự án, và một sổ đọc đường dẫn từ cấu hình khởi động thì lần dựng thứ hai vẫn
/// ra đường dẫn cũ — im lặng, và chỉ lộ ra khi người dùng thấy tool đọc nhầm repo.
fn catalog(
    config: &Config,
    workspace: &Path,
    llm: Arc<ActiveLlm>,
    embedder: Arc<ActiveEmbedder>,
    sessions: SessionService,
) -> PluginCatalog {
    let mut catalog = PluginCatalog::new();
    let identity = IDENTITY.to_string();
    let workspace = workspace.to_path_buf();
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
            // Ba nguồn, quét theo thứ tự và nguồn sau **thay thế** gói trùng tên của nguồn
            // trước: bộ dựng sẵn đi kèm bản cài đặt, gói của người dùng trong kho dữ liệu,
            // rồi gói riêng của dự án nằm ngay trong repo. Thứ tự ấy là một thang thẩm
            // quyền — một repo nói khác đi về quy trình của chính nó thì nó đúng, và người
            // dùng đè lên bộ dựng sẵn thì họ đúng.
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
            // Chỉ thư mục làm việc được cấp quyền. Kho dữ liệu của chính ứng dụng thì
            // không: mô hình không có việc gì trong đó, và cấp quyền "cho tiện" là cách
            // một tệp thiết lập bị sửa bởi một câu trong tài liệu người dùng vừa nạp.
            // Đường dẫn lấy từ chính `pai-mcp`, không viết tay lại: tệp này là chìa
            // khoá của mọi tool khác, và hai chuỗi ở hai crate thì sớm muộn trôi ra
            // khỏi nhau mà không ai nhận ra cho tới lúc nó đọc được.
            Ok(
                Box::new(FsPlugin::new([workspace.clone()], [token_path(&data_dir)]))
                    as Box<dyn Plugin>,
            )
        });
    }
    catalog.register("hooks", |value| {
        let row: HooksRow = serde_json::from_value(value.clone())?;
        Ok(Box::new(HooksPlugin::new(row.hooks)) as Box<dyn Plugin>)
    });
    {
        let (workspace, model) = (workspace.clone(), config.model.clone());
        catalog.register("subagent", move |_| {
            // Con trỏ chia sẻ, không phải bản sao adapter: agent con phải đi tới cùng
            // provider mà lượt cha đang dùng, kể cả khi người dùng vừa đổi provider.
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
        let data_dir = data_dir.clone();
        catalog.register("rag", move |_| {
            // Kho tài liệu nằm trong **kho dữ liệu của ứng dụng**, không trong thư mục dự
            // án, dù thư mục dự án thoạt nghe là chỗ tự nhiên hơn. Ba lý do, và cả ba đều
            // là chuyện đã xảy ra với người dùng thật ở các sản phẩm khác:
            //
            //   - Thư viện giữ một **bản sao** của mọi tài liệu. Đổ nó vào thư mục dự án
            //     là nhân đôi dung lượng ngay trước mắt người dùng, trong đúng thư mục họ
            //     vừa thả hai mươi tệp PDF vào.
            //   - Bản sao đó là nguồn dựng lại chỉ mục. Nằm trong thư mục người dùng thì
            //     một lần dọn tay sẽ làm tài liệu biến mất khỏi thư viện mà không ai báo.
            //   - `Library::remove` xoá bản sao. Xoá tệp trong thư mục của người dùng là
            //     việc một thư viện không nên tự làm, kể cả khi tệp đó do nó tạo ra.
            let embedder: Option<Arc<dyn Embedder>> = Some(embedder.clone());
            Ok(Box::new(RagPlugin::new(
                data_dir
                    .join("du-an")
                    .join(project_slug(&workspace))
                    .join("tai-lieu"),
                embedder,
            )) as Box<dyn Plugin>)
        });
    }
    catalog.register("sandbox", |_| {
        Ok(Box::new(SandboxPlugin::new()) as Box<dyn Plugin>)
    });
    {
        let (data_dir, workspace) = (data_dir.clone(), workspace.clone());
        catalog.register("index", move |_| {
            // Cùng bộ gốc và cùng danh sách bảo vệ với `fs`: một chỉ mục nhìn rộng hơn
            // tool đọc là một đường vòng qua đúng ranh giới mà `fs` dựng lên.
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
            // Cùng bộ gốc và cùng danh sách bảo vệ với `fs` và `index`. Không có
            // language server nào trên máy thì plugin cắm xong mà **không đăng ký tool
            // nào** — đó là trạng thái hợp lệ, không phải lỗi khởi động.
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
            Ok(Box::new(TerminalPlugin::new(workspace.clone())) as Box<dyn Plugin>)
        });
    }
    catalog.register("shell", move |_| {
        Ok(Box::new(ShellPlugin::new(workspace.clone())) as Box<dyn Plugin>)
    });
    {
        let data_dir = data_dir.clone();
        catalog.register("mcp", move |value| {
            let row: McpRow = serde_json::from_value(value.clone())?;
            let mut plugin = McpPlugin::new(row.servers).storing(data_dir.join("mcp.json"));
            // Phơi ra ngoài **tắt mặc định**. Mở một cổng, kể cả cổng loopback, là một
            // hành động hướng ra ngoài; nó phải là thứ người dùng bật, không phải thứ họ
            // phát hiện ra là đang chạy.
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

/// Cấu hình của hàng `hooks`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct HooksRow {
    hooks: Vec<HookConfig>,
}

/// Cấu hình của hàng `mcp`.
///
/// Khai ở đây chứ không trong `pai-mcp` vì đây là hình dạng của **một hàng cấu hình**,
/// và hàng cấu hình là chuyện của chỗ dựng cây, không phải của crate làm việc thật.
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

/// Các lớp cấu hình, theo đúng thứ tự áp: nền trước, người dùng sau.
///
/// Một lần áp cho cả cây; việc chia tầng xảy ra **sau**, trên danh sách hàng đã áp xong.
/// Nhờ thế bản vá của người dùng không phải biết gì về chuyện chia tầng, và một `id` gõ
/// sai vẫn dừng khởi động như trước.
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
            // Tệp vá hỏng thì **dừng khởi động**, không bỏ qua: chạy tiếp với cây mặc
            // định trông y hệt chạy đúng, và người dùng sẽ đi tìm xem vì sao bản vá của
            // họ không có tác dụng.
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

    // Sổ phiên và adapter mô hình dựng **trước** vòng lặp plugin, vì `subagent` là một
    // plugin cần cả hai: một agent con là một lượt trọn vẹn, nên nó cần đúng những gì một
    // lượt cần. Chúng vẫn không phải plugin — chúng là thứ plugin dùng.
    let store: Arc<dyn SessionStore> =
        Arc::new(SqliteSessionStore::open(config.data_dir.join("phien.db"))?);
    let sessions = SessionService::new(store);
    let http = reqwest::Client::new();

    // Kho provider, và một lần gieo nếu nó rỗng.
    //
    // Cấu hình môi trường (`PAI_OLLAMA_URL`, `PAI_MODEL`) không biến mất — nó trở thành
    // **hàng đầu tiên trong kho** thay vì một đường đi song song. Nhờ vậy có đúng một
    // nguồn sự thật cho "đang nói chuyện với ai", và người dùng sửa được nó từ trong ứng
    // dụng thay vì phải đi tìm biến môi trường đã đặt ở đâu.
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
        providers.activate(seeded.id(), Some(&config.model))?;
    }

    // Con trỏ tới provider đang hoạt động. Dựng **trước** vòng lặp plugin vì `subagent`
    // cần nó lúc cắm, và dựng nó rỗng rồi điền sau nghĩa là có một khoảng thời gian nó
    // trỏ vào hư không.
    let boot_adapter: Arc<dyn LlmAdapter> = Arc::new(OllamaAdapter::new(
        "ollama",
        &config.ollama_url,
        http.clone(),
    ));
    let llm = Arc::new(ActiveLlm::new(boot_adapter));
    let embedder = Arc::new(ActiveEmbedder::empty());

    let projects: Arc<dyn ProjectStore> =
        Arc::new(SqliteProjectStore::open(config.data_dir.join("du-an.db"))?);
    // Thư mục khởi động là một dự án như mọi dự án khác, chỉ khác ở chỗ nó được mở sẵn.
    let project = projects.touch(&config.workspace)?;

    let composed = compose(&layers(&config)?)?;
    // Rút danh sách server của hàng `mcp` **một lần**, ngay chỗ cây cấu hình còn nguyên.
    let mcp_rows = composed
        .active()
        .find(|row| row.plugin == "mcp")
        .and_then(|row| serde_json::from_value::<McpRow>(row.config.clone()).ok())
        .map(|row| row.servers)
        .unwrap_or_default();
    tracing::debug!("cây plugin:\n{}", composed.dump());
    let catalog = catalog(
        &config,
        Path::new(&project.path),
        llm.clone(),
        embedder.clone(),
        sessions.clone(),
    );

    // Thứ tự trong danh sách không quyết định thứ tự nạp — phụ thuộc được diễn đạt bằng
    // `require`, nên `fs` chờ `tools` có mặt chứ không chờ nó đứng trước.
    //
    // Giữ scope thay vì `mem::forget` nó. Thả trôi thì việc dọn bất đồng bộ của mọi plugin
    // — đóng client MCP, tắt language server, giết job nền — không bao giờ có cơ hội chạy,
    // và cái đó chỉ lộ ra dưới dạng tiến trình mồ côi sau khi đóng app.
    let mut scopes = Vec::new();
    let mut project_scopes = Vec::new();
    for row in composed.active() {
        // Bỏ qua trước cả khi dựng: một hàng `rag` trong dự án mã nguồn không có gì để
        // dựng, và gọi `build` rồi vứt đi là mở một cơ sở dữ liệu chẳng ai đọc.
        if thuoc_du_an(row) && !hop_loai(row, project.kind) {
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

    // Tầng provider dựng **sau** `Driver` vì nó cần chính cái `Driver` ấy để đẩy adapter
    // vào. Nó không phải một hàng plugin vì lý do đó: một plugin được cắm trước khi
    // `Driver` tồn tại, nên nó sẽ phải nhận một `Driver` chưa có — và một seam nhận giá
    // trị điền sau là một seam có một khoảng thời gian trả lời sai.
    let runtime = Arc::new(ProviderRuntime::new(
        providers.clone(),
        Arc::new(AdapterRegistry::new(http.clone())),
        driver.clone(),
        http.clone(),
    ));
    // Giao guard cho scope gốc thay vì giữ nó trong `Harness`: `Guard` bọc một
    // `Box<dyn FnOnce + Send>` nên nó không `Sync`, mà `Harness` thì nằm trong `State` của
    // Tauri và phải `Sync`. Scope gốc sống bằng tiến trình, nên vòng đời không đổi — chỉ
    // đổi chỗ cất.
    ctx.keep(ctx.provide::<Providers>(runtime.clone())?);
    // Máy chưa cấu hình gì là trạng thái bình thường lúc mới cài, không phải lỗi khởi
    // động: ứng dụng vẫn mở, và màn hình provider là chỗ người dùng sửa nó.
    if let Err(err) = runtime.apply_active().await {
        tracing::warn!("chưa dùng được nhà cung cấp nào: {err}");
    }
    apply_llm(&runtime, &llm, &embedder, &config);
    Ok(Harness {
        ctx: ctx.clone(),
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
            embedder: embedder.clone(),
            sessions,
            composed,
        },
        llm,
        embedder,
    })
}
