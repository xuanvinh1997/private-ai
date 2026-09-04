//! The contract between core and UI; the TypeScript copy lives in `ui/src/lib/protocol.ts` and is matched by
//! hand, so every change here needs the matching change there in the same commit -- a mismatch fails at
//! runtime, not at compile time. `rename_all = "snake_case"` renames variants only.

// A wire contract, not application code: an unconstructed variant means that core path is not wired up yet, not that it is dead.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// One diff hunk; `old_text: None` means a new file, not "nothing changed".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
    /// First line number in the real file; without it the UI numbers from 1, which is the line within the hunk.
    pub old_start: Option<u32>,
    pub new_start: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadLine {
    pub number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMeta {
    pub path: String,
    pub offset: u32,
    pub lines: Vec<ReadLine>,
    pub total_lines: u32,
    pub lang: Option<String>,
    /// Truncated to fit the budget; without this the UI cannot tell a whole file from its head and tail, and a
    /// reader concludes "that is all" exactly where the core stopped.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchGroup {
    pub path: String,
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchShape {
    Matches,
    Paths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMeta {
    pub shape: SearchShape,
    /// Result truncated for display; the full version is in the spill store, not lost.
    pub truncated: bool,
    pub total: u32,
    pub groups: Option<Vec<SearchGroup>>,
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalMeta {
    pub command: String,
    pub cwd: Option<String>,
    pub output: String,
    /// A background job has no exit code yet, which does not mean it hung.
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub background: bool,
    pub job_id: Option<String>,
}

/// What rides along with a tool result so the UI can draw a rich card; the UI renders from the raw event with
/// no presentation API in between to drift.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<DiffHunk>>,
    /// The ticket for retrieving full output truncated to fit the token budget; the model uses `spill_read`,
    /// the UI draws a full view. Without this field serde silently drops the key the tool wrote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill: Option<SpillMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalMeta>,
}

/// The full-text retrieval ticket; mirrors `pai_tools::SpillRef`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpillMeta {
    pub id: String,
    pub tool: String,
    /// Full-text size, in Unicode characters.
    pub chars: u64,
    pub lines: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub status: TodoStatus,
}

/// An approval decision, with exactly two values: there is no "remember this" here, since one yes is one yes, not a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowedOnce,
    Rejected,
}

/// Tool permissions the user grants for one turn: sent with each message rather than stored, because lowering
/// privilege for a single question is how the picker is actually used and a sticky setting is one people forget.
/// Variants are ordered from most to least restrictive; the mapping lives in `crate::scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolScope {
    /// Only tools that declare `mutating: false`.
    Read,
    /// Adds file-editing tools; no command execution.
    Write,
    /// Everything, including executing commands on this machine.
    Shell,
}

/// One event in the life of a turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// A fragment of assistant text; the densest event here, so tokens are coalesced on the Rust side before crossing IPC.
    Token {
        text: String,
    },
    Progress {
        label: String,
        detail: Option<String>,
    },
    Notice {
        message: String,
    },
    ToolStart {
        call_id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolEnd {
        call_id: String,
        name: String,
        /// A tool-level error the model can read. Not a panic.
        is_error: bool,
        preview: String,
        // Boxed because it outweighs every other variant combined while `Token` is the one built thousands of times; the wire is unchanged.
        meta: Option<Box<ToolMeta>>,
    },
    /// The intended diff, emitted as the tool starts so the user sees a change before it happens; a shortcut for
    /// tools whose `args` are not enough to derive one.
    Diff {
        call_id: String,
        diffs: Vec<DiffHunk>,
    },
    /// The whole todo list, sent complete each time, so the UI never folds state and the two ends cannot diverge.
    Todo {
        items: Vec<TodoItem>,
    },
    /// The core asks the UI; no answer means denial.
    ApprovalRequest {
        request_id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
        reason: Option<String>,
        timeout_ms: Option<u64>,
    },
    /// Withdraw the question because the turn was cancelled; the UI closes the dialog.
    ApprovalCancel {
        request_id: String,
    },
    /// Tokens for the step just finished, plus the running model's context window, so the UI can show context
    /// pressure while it rises. `contextWindow` is `None` when the model cannot be asked, and a bar without a
    /// denominator says nothing.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        context_window: Option<u64>,
    },
    Final {
        message_id: String,
    },
    Error {
        message: String,
    },
}

/// One session in the sidebar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// Epoch milliseconds.
    pub updated_at: i64,
    /// The last thing said in the session; `None` must render as a one-line row, not two with a blank second line.
    pub preview: Option<String>,
}

impl SessionSummary {
    pub fn from_header(header: pai_session::SessionHeader) -> SessionSummary {
        SessionSummary::with_preview(header, None)
    }

    pub fn with_preview(
        header: pai_session::SessionHeader,
        preview: Option<String>,
    ) -> SessionSummary {
        SessionSummary {
            preview,
            // An untitled session must still appear in the list; a blank row is an unclickable row.
            title: header.title.unwrap_or_else(|| "Phiên mới".to_string()),
            id: header.id,
            updated_at: header.updated_at,
        }
    }
}

/// A node in a stored transcript, rebuilt from the session log; it shares the `kind` vocabulary with the UI's
/// `ConversationNode`, so a reloaded transcript and a live turn render through the same code.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryNode {
    User {
        id: String,
        text: String,
        created_at: i64,
    },
    Assistant {
        id: String,
        text: String,
        created_at: i64,
    },
    Tool {
        id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
        is_error: bool,
        preview: String,
        // Boxed for the same reason as `AgentEvent::ToolEnd`: it outweighs the other two, and a long transcript is mostly messages.
        meta: Option<Box<ToolMeta>>,
        created_at: i64,
    },
}

/// A model the server currently offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    /// Whether it can call tools; a coding agent is useless without that, so the UI must say so before a wrong choice.
    pub tools: bool,
    /// Chat-capable.
    pub chat: bool,
    /// Embedding-capable. Two flags rather than an enum because they are not exclusive: only
    /// `embedding && !chat` is hidden from the chat picker, since filtering on `chat` would erase usable models
    /// on older Ollama servers where capability has to be inferred.
    pub embedding: bool,
    /// Sees images. Authoritative where the server declares it (Ollama `/api/show`, LM Studio), a name guess
    /// otherwise -- so the vision picker orders by it and never filters on it.
    pub vision: bool,
    pub context_window: Option<u64>,
}

/// One project in the sidebar; `is_current` is UI-only because the store does not know which project is open,
/// and runtime state in a stored row ends up written to disk and wrong next launch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_opened_at: i64,
    pub is_current: bool,
    pub kind: ProjectKind,
    /// The URL it was cloned from; `None` means a directory that already existed locally.
    pub origin: Option<String>,
}

impl ProjectView {
    /// `current` is `None` when no project is open -- a valid state; an empty string would give the right answer by accident.
    pub fn new(project: pai_project::Project, current: Option<&str>) -> ProjectView {
        ProjectView {
            is_current: current.is_some_and(|id| id == project.id),
            kind: project.kind.into(),
            origin: project.origin,
            id: project.id,
            name: project.name,
            path: project.path,
            last_opened_at: project.last_opened_at,
        }
    }
}

/// Two enums with the same wire strings, one per layer: the store must not know about the UI, so the project
/// kind exists twice and this is the bridge.
impl From<pai_project::ProjectKind> for ProjectKind {
    fn from(kind: pai_project::ProjectKind) -> ProjectKind {
        match kind {
            pai_project::ProjectKind::Code => ProjectKind::Code,
            pai_project::ProjectKind::Docs => ProjectKind::Docs,
        }
    }
}

impl From<ProjectKind> for pai_project::ProjectKind {
    fn from(kind: ProjectKind) -> pai_project::ProjectKind {
        match kind {
            ProjectKind::Code => pai_project::ProjectKind::Code,
            ProjectKind::Docs => pai_project::ProjectKind::Docs,
        }
    }
}

// Project kinds, document library, providers and MCP: four groups added together because they are one product
// change -- a project is now code or documents, and that decides tools, screens and models.

/// Whether a project is source code or a pile of documents; not a filter label but the choice of plugin layer.
/// Document projects get `rag` and nothing that executes, since their files came from other people.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Code,
    Docs,
}

/// `git clone` progress, emitted on a `Channel` while the command runs; `percent` is absent in phases git
/// cannot count, where a frozen 0% bar is worse than a line of text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneProgress {
    /// The phase, named by git itself: counting objects, receiving, resolving deltas.
    pub phase: String,
    pub percent: Option<u8>,
    /// The raw line, kept for the details pane when something goes wrong.
    pub line: Option<String>,
    pub finished: bool,
    /// The finished directory; present only on the last event, and only on success.
    pub path: Option<String>,
    pub error: Option<String>,
}

/// One document in a document project's library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub id: String,
    /// Path to the copy in the project's store, not where the user got it from.
    pub path: String,
    pub title: String,
    /// `pdf`, `docx`, `markdown`, `text`, `html`, `csv`, `code`.
    pub format: String,
    pub bytes: u64,
    pub chunks: u32,
    pub pages: u32,
    pub ocr_pages: Vec<u32>,
    /// Whether vectors exist; `false` with no `error` means queued, not broken, and keyword search still works.
    pub embedded: bool,
    pub added_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSetting {
    pub enabled: bool,
    pub vision_model: Option<String>,
}

/// Progress of ingesting documents into the library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestProgress {
    /// The file being processed.
    pub path: String,
    pub stage: String,
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    pub error: Option<String>,
}

/// Document library health, enough for the UI to explain why answers are poor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub documents: u32,
    pub chunks: u32,
    pub embedded_chunks: u32,
    /// The embedding model in use; `None` means unconfigured, and search falls back to keywords rather than returning nothing.
    pub embedder: Option<String>,
    pub semantic_ready: bool,
    /// The explanation shown when `semantic_ready` is false, or when the library is empty while the directory is not.
    pub reason: Option<String>,
    /// The user's document directory, which the UI must show: "why are there no files" starts with checking where they pointed.
    pub root: String,
    pub files_seen: u32,
    /// Skipped for hitting a limit -- file size or file count.
    pub files_skipped: u32,
    pub unreadable: u32,
    /// Still in the directory but removed from the library by the user.
    pub excluded: u32,
    /// Last scan, epoch milliseconds; `None` means never scanned, which differs from scanned-and-empty.
    pub scanned_at: Option<i64>,
    /// Scan in progress as `(done, total)`; `None` means no scan is running.
    pub scanning: Option<ScanProgress>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub done: u32,
    pub total: u32,
}

/// Material the UI turns into empty-screen suggestions: facts about the open project only, never phrasing,
/// which lives with the static sets in `ui/src/lib/prompts.ts`. All three fields empty is a valid state, and
/// the UI falls back to the static set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptSeeds {
    /// Most-connected symbols first, in `code.overview` order; names only, since a chip a few words wide cannot hold a path.
    pub symbols: Vec<String>,
    /// Directories with the most symbols first.
    pub directories: Vec<String>,
    /// Document names in the library; document projects only.
    pub documents: Vec<String>,
}

/// One matching passage, enough to build a citation card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentHit {
    pub document_id: String,
    pub title: String,
    pub path: String,
    pub ordinal: u32,
    pub text: String,
    pub score: f32,
    /// `keyword`, `semantic` or `both`; the reader needs to know why this passage was chosen.
    pub matched_by: String,
}

/// A configured provider as the UI sees it, carrying no API key: `has_key` is enough to decide whether the
/// field shows "set", and a key crossing IPC is a key in every open debugging tool's log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    /// `ollama` or `openai`.
    pub kind: String,
    pub base_url: String,
    pub has_key: bool,
    pub enabled: bool,
    /// The endpoint never leaves loopback: nothing goes anywhere, and the UI says so.
    pub on_device: bool,
    /// Currently used for chat.
    pub active_chat: bool,
    /// Currently used for embedding documents. The roles are fully separate because embedding and chat are
    /// different models on different endpoints, and the most useful pairing is cross-wired: embed locally while
    /// chatting with a large remote model.
    pub active_embedding: bool,
    /// Currently used to read images and scanned PDF pages.
    pub active_vision: bool,
    /// The chat model chosen for this provider.
    pub model: Option<String>,
    /// The embedding model chosen for this provider.
    pub embedding_model: Option<String>,
    pub vision_model: Option<String>,
}

/// The embedding configuration in effect, provider and model combined, because "what embeds my documents, and does it work" is one question.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSetting {
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    /// Documents never leave this machine while embedding.
    pub on_device: bool,
    /// The sentence explaining why it is unavailable, when it is.
    pub reason: Option<String>,
}

/// The vision configuration in effect: which provider reads images, under which model, and why it cannot.
/// Separate from [`EmbeddingSetting`] because reading a scanned page sends the whole page somewhere, while
/// embedding sends text -- the privacy question is asked once per role, not once per screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionSetting {
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    /// Page images never leave this machine while reading.
    pub on_device: bool,
    /// The sentence explaining why OCR cannot read images, when it cannot.
    pub reason: Option<String>,
    /// The OCR switch itself, so one screen answers "will scans be read" without a second round trip.
    pub ocr_enabled: bool,
}

/// One real OCR attempt on a bundled test image.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionProbe {
    pub ok: bool,
    pub message: String,
    /// What the model returned, so a wrong read is visible as a wrong read rather than a bare failure.
    pub text: Option<String>,
}

/// Local ONNX reranking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankSetting {
    /// Off still retrieves, just less well. See `reason`.
    pub enabled: bool,
    /// Currently always `onnx`.
    pub backend: String,
    /// Fixed local cross-encoder model.
    pub model: String,
    /// How many candidates to fetch for rescoring -- the latency dial.
    pub candidates: u32,
    /// How many to keep after scoring.
    pub top_n: u32,
    /// The sentence stating the current cost, shown next to the toggle so the trade is visible.
    pub reason: Option<String>,
}

/// The result of really embedding a sentence, not of listing models: listing cannot say which model embeds,
/// so this sends text and reports the dimensions that come back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingProbe {
    pub ok: bool,
    pub message: String,
    /// Dimensions measured from the vector actually returned.
    pub dimensions: Option<usize>,
}

/// One built-in entry in the provider catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub needs_key: bool,
    pub on_device: bool,
    pub default_model: Option<String>,
    /// Where to get a key, or where to download the server.
    pub homepage: String,
    pub hint: String,
}

/// The result of probing a provider configuration before saving it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbe {
    pub ok: bool,
    pub message: String,
    pub models: Vec<ModelChoice>,
}

/// One MCP server as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub name: String,
    /// `stdio` or `http`.
    pub transport: String,
    /// Command line or URL, shortened to fit one line.
    pub target: String,
    pub enabled: bool,
    /// `connected`, `connecting`, `failed`, `disabled`.
    pub state: String,
    /// Attached tool names, already prefixed with `ext.<name>.`.
    pub tools: Vec<String>,
    pub error: Option<String>,
}

/// An environment variable a catalogue entry needs the user to fill in.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpEnvVar {
    pub key: String,
    pub label: String,
    pub required: bool,
    /// Masked while typing, and never sent back to the UI after saving.
    pub secret: bool,
}

/// A built-in server the user attaches with one click.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCatalogEntry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<McpEnvVar>,
    pub homepage: String,
    /// What must exist locally (`node`, `python`, `docker`); the UI warns before the click, not after a `failed` server.
    pub requires: Vec<String>,
    /// The endpoint of a remotely hosted server, if this entry is one; then `command`, `args` and `requires` are
    /// empty and no child process is spawned, which the UI must say -- "nothing to install" is why people pick it.
    pub url: Option<String>,
}

/// One node in the source graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeView {
    pub id: String,
    pub name: String,
    /// `function`, `method`, `struct`, `class`, `trait`, `interface`, `enum`, `module`,
    /// `constant`, `type`.
    pub kind: String,
    pub path: String,
    pub line: u32,
}

/// One edge; `kind` is `calls`, `imports`, `contains`, `implements`, `extends` or `references`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeView {
    pub src: String,
    pub dst: String,
    pub kind: String,
}

/// A slice of the graph, small enough to draw.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView {
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    /// Truncated to stay drawable: a node with four hundred edges renders as a black blob.
    pub truncated: bool,
}

/// Source index status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub files: u32,
    pub symbols: u32,
    pub edges: u32,
    /// `(language, file count)`, largest first.
    pub languages: Vec<(String, u32)>,
    /// Last scan, epoch milliseconds.
    pub scanned_at: Option<i64>,
}

/// A provider configuration sent up from the UI. `api_key` is three-state: `None` keeps the stored key,
/// `Some("")` clears it, `Some(k)` sets it. The UI never receives the key back, so merging the first two cases
/// would drop it on every rename.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInputWire {
    pub id: Option<String>,
    pub name: String,
    /// `ollama` or `openai`.
    pub kind: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub model: Option<String>,
    pub embedding_model: Option<String>,
    pub vision_model: Option<String>,
}

/// An MCP server sent up from the UI.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpServerInputWire {
    pub name: String,
    /// `stdio` or `http`.
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub enabled: bool,
}

/// The process sandbox as the UI sees it: a report, not a promise. `mode` says how much the kernel enforces
/// and `reason` says where it leaks, because a silent permissions screen teaches trust in a boundary that may not exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// `full`, `partial` or `none`.
    pub mode: String,
    /// Why it leaks, or why there is nothing; `None` when `mode` is `full`.
    pub reason: Option<String>,
    /// The directory commands may write to.
    pub writable_roots: Vec<String>,
    /// `macos`, `linux` or `windows`: confinement differs per platform, and the reader needs to know which one they are on.
    pub platform: String,
}

/// One installed hook, read-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRow {
    pub command: String,
    /// The tools this hook applies to; empty means all of them.
    pub tools: Vec<String>,
    /// Its own timeout in seconds; `None` uses the core default.
    pub timeout_secs: Option<u64>,
    /// The configuration layer that declared it: built-in, or the user's patch file.
    pub origin: String,
}

/// One entry in the project tree; no size or mtime, because this tree is for reaching a file, not auditing a disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryView {
    pub name: String,
    /// An absolute path; the UI sends it back verbatim when expanding a subdirectory, so it must be relative to nothing.
    pub path: String,
    pub is_dir: bool,
}
