/** TypeScript mirror of `AgentEvent` in app/src/lib.rs; the two sides are matched by hand, so change both in
 * one commit. Variants below `Error` are declared ahead of Rust; wire field names stay snake_case. */

/** One diff hunk. `old_text: null` means a new file, not "unchanged". */
export interface DiffHunk {
  path: string;
  old_text: string | null;
  new_text: string;
  /** First line of the old/new side in the real file; absent, the block numbers from 1 within the hunk. */
  old_start?: number | null;
  new_start?: number | null;
}

export interface ReadLine {
  number: number;
  text: string;
}

export interface SearchMatch {
  line: number;
  text: string;
}

export interface SearchGroup {
  path: string;
  matches: SearchMatch[];
}

/** `tool/result.data.meta`, the only transported input for rich cards: the UI renders raw events, with no present API. */
/** Ticket for retrieving the full text when output was cut to fit the token budget. */
export interface SpillMeta {
  id: string;
  tool: string;
  /** Full-text size, in Unicode characters. */
  chars: number;
  lines: number;
}

export interface ToolMeta {
  diffs?: DiffHunk[];
  /** Ticket for the full text when output was truncated; the model reads it with `spill_read`. */
  spill?: SpillMeta;
  read?: {
    path: string;
    offset: number;
    lines: ReadLine[];
    total_lines: number;
    lang?: string | null;
    /** Truncated; without this flag a partial read looks exactly like a complete one. */
    truncated?: boolean;
  };
  search?: {
    shape: "matches" | "paths";
    truncated: boolean;
    total: number;
    groups?: SearchGroup[];
    paths?: string[];
  };
  terminal?: {
    command: string;
    cwd?: string | null;
    output: string;
    exit_code: number | null;
    signal?: string | null;
    /** Background command: a missing exit code does not mean it hung. */
    background?: boolean;
    job_id?: string | null;
  };
}

export type TodoStatus = "pending" | "in_progress" | "done" | "cancelled";

export interface TodoItem {
  id: string;
  text: string;
  status: TodoStatus;
}

/** Approval decision; only two values, with no "remember my choice" in the vocabulary. */
export type ApprovalDecision = "allowed_once" | "rejected";

/** Tool permission for *one turn*, mirroring `ToolScope` in app/src/protocol.rs; the core enforces it at both layers. */
export type ToolScope = "read" | "write" | "shell";

export type AgentEvent =
  | { kind: "token"; text: string }
  | { kind: "progress"; label: string; detail: string | null }
  | { kind: "notice"; message: string }
  | { kind: "tool_start"; call_id: string; name: string; args: unknown }
  | {
      kind: "tool_end";
      call_id: string;
      name: string;
      is_error: boolean;
      preview: string;
      /** NOT yet on the Rust side. The source of every rich card (diff, terminal, read, search). */
      meta?: ToolMeta | null;
    }
  /** Tokens for the step just finished; `context_window` is `null` when unknown, so only the count is shown. */
  | {
      kind: "usage";
      input_tokens: number;
      output_tokens: number;
      context_window: number | null;
    }
  | { kind: "final"; message_id: string }
  | { kind: "error"; message: string }
  /** NOT yet on the Rust side: the *intended* diff, a shortcut for tools whose args cannot yield one (see `diff.ts`). */
  | { kind: "diff"; call_id: string; diffs: DiffHunk[] }
  /** NOT yet on the Rust side: the todo list, sent whole each time so the UI never folds state. */
  | { kind: "todo"; items: TodoItem[] }
  /** NOT yet on the Rust side: the host asks the UI. No answer means refusal (see `agent.ts`). */
  | {
      kind: "approval_request";
      request_id: string;
      call_id: string;
      name: string;
      args: unknown;
      reason: string | null;
      timeout_ms: number | null;
    }
  /** NOT yet on the Rust side: the host withdraws the question (turn cancelled); the UI closes the dialog. */
  | { kind: "approval_cancel"; request_id: string };

/** A tool call, running or finished, built from the tool_start/tool_end pair. */
export interface ToolCall {
  callId: string;
  name: string;
  args: unknown;
  state: "running" | "ok" | "error";
  preview?: string;
  meta?: ToolMeta;
  /** Intended diff while running; replaced by `meta.diffs` once the tool finishes. */
  intendedDiffs?: DiffHunk[];
}

/** One row of the transcript and the registry's dispatch unit: `kind` is the renderer key. */
export type ConversationNode =
  | { id: string; kind: "user"; text: string; at?: number }
  | { id: string; kind: "assistant"; text: string; streaming: boolean; at?: number }
  | { id: string; kind: "tool"; call: ToolCall; at?: number }
  | { id: string; kind: "notice"; message: string }
  | { id: string; kind: "progress"; label: string; detail: string | null }
  | { id: string; kind: "error"; message: string }
  | { id: string; kind: "todo"; items: TodoItem[] };

export type NodeKind = ConversationNode["kind"];

/** A session in the sidebar. `updatedAt` is epoch ms. */
export interface SessionSummary {
  id: string;
  title: string;
  /** Last thing said in the session; `null` means the row must be one line, not two with an empty second. */
  preview: string | null;
  updatedAt: number;
}

/** A node from the stored session log; it shares `kind` with `ConversationNode` so one renderer set covers both. */
export type HistoryNode =
  | { kind: "user"; id: string; text: string; created_at: number }
  | { kind: "assistant"; id: string; text: string; created_at: number }
  | {
      kind: "tool";
      id: string;
      call_id: string;
      name: string;
      args: unknown;
      is_error: boolean;
      preview: string;
      meta: ToolMeta | null;
      created_at: number;
    };

/** A model the server offers. */
export interface ModelChoice {
  id: string;
  /** Whether it can call tools; without that a coding agent is silently useless, so the UI must say so up front. */
  tools: boolean;
  /** Can chat. */
  chat: boolean;
  /** Can embed; the flags are not exclusive, and only `embedding && !chat` is hidden from the chat model picker. */
  embedding: boolean;
  /** Can see images. Authoritative where the server declares it, a name guess otherwise — so it orders the
   * vision picker and never filters it. */
  vision: boolean;
  contextWindow: number | null;
}

/** A project in the recent list; `isCurrent` is set by the core, since only it knows when the plugins finished swapping. */
export interface Project {
  id: string;
  name: string;
  path: string;
  lastOpenedAt: number;
  isCurrent: boolean;
  kind: ProjectKind;
  /** Clone source URL; `null` means a directory that was already on the machine. */
  origin: string | null;
}

/* --- Project kinds, document library, providers and MCP; mirrored by hand from `app/src/protocol.rs`. --- */

/** Source code or a pile of documents; not a filter label but the choice of which plugin layer gets attached. */
export type ProjectKind = "code" | "docs";

/** `git clone` progress; `percent` is absent in phases git cannot count. */
export interface CloneProgress {
  phase: string;
  percent: number | null;
  line: string | null;
  finished: boolean;
  path: string | null;
  error: string | null;
}

export type DocumentFormat =
  | "pdf"
  | "office"
  | "image"
  | "audio"
  | "markdown"
  | "text"
  | "html"
  | "data"
  | "code";

/** Raw facts about the open project for empty-screen suggestions; the wording itself lives in `lib/prompts.ts`. */
export interface PromptSeeds {
  /** Symbols with the most relations first; names only, no paths. */
  symbols: string[];
  /** Directories with the most symbols first. */
  directories: string[];
  /** Document titles in the library; docs projects only. */
  documents: string[];
}

/** A document in the library. */
export interface DocumentView {
  id: string;
  path: string;
  title: string;
  format: DocumentFormat;
  bytes: number;
  chunks: number;
  pages: number;
  ocrPages: number[];
  /** Whether vectors exist; `false` with `error === null` means queued, not failed. */
  embedded: boolean;
  addedAt: number;
  error: string | null;
}

export interface OcrSetting {
  enabled: boolean;
  visionModel: string | null;
}

export interface IngestProgress {
  path: string;
  /** One of `reading` `ocr` `transcribing` `stored` `failed` `skipped` `removed` `embedding` `finished`. */
  stage: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
}

/** Library health, enough to say *why* answers are poor rather than just that they are. */
export interface LibraryStats {
  documents: number;
  chunks: number;
  embeddedChunks: number;
  embedder: string | null;
  semanticReady: boolean;
  reason: string | null;
  /** The user's document directory; the screen must show it, since "no files" starts with checking the path. */
  root: string;
  filesSeen: number;
  /** Skipped for hitting a limit, either file size or the file-count cap. */
  filesSkipped: number;
  unreadable: number;
  /** Still in the directory but removed from the library by the user. */
  excluded: number;
  /** `null` means never scanned, which is not the same as scanned and empty. */
  scannedAt: number | null;
  scanning: { done: number; total: number } | null;
}

export interface DocumentHit {
  documentId: string;
  title: string;
  path: string;
  ordinal: number;
  text: string;
  score: number;
  matchedBy: "keyword" | "semantic" | "both";
}

export type ProviderKind = "ollama" | "lmstudio" | "openai";

/** A configured provider, never carrying the API key: `hasKey` is all the UI needs, and a key on IPC is a leaked key. */
export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  hasKey: boolean;
  enabled: boolean;
  onDevice: boolean;
  /** Currently used for *chat*. */
  activeChat: boolean;
  /** Currently used for *embedding*; the roles are separate because local embedding with remote chat is the common pairing. */
  activeEmbedding: boolean;
  /** Currently used for OCR. */
  activeVision: boolean;
  /** Chat model. */
  model: string | null;
  /** Embedding model. */
  embeddingModel: string | null;
  /** Image-reading model used for scanned PDFs and image files. */
  visionModel: string | null;
}

/** How documents are cut before embedding; changing either number re-cuts and re-embeds the whole library. */
export interface ChunkSetting {
  /** Target characters per chunk. */
  size: number;
  /** Characters repeated from the previous chunk, so a sentence split across the seam stays findable. */
  overlap: number;
  /** The sentence naming the trade-off at these numbers. */
  reason: string | null;
}

/** Local ONNX rerank settings. */
/** What a loaded speech model turned out to be. */
export interface AsrModelInfo {
  arch: string;
  variant: string;
  /** The compute backend it landed on: Metal, CPU, and so on. */
  backend: string;
  /** False for a model that only transcribes finished audio: dictation then produces its text at the end. */
  streaming: boolean;
  languages: string[];
}

export interface AsrSetting {
  /** Read audio files found in a document project. Off leaves them out of the library entirely. */
  enabled: boolean;
  /** Path of the chosen `.gguf`, or empty when none is chosen. */
  model: string;
  /** Language hint; empty means let the model decide. */
  language: string;
  /** Filled in only after a probe, which is what pays the load. */
  info: AsrModelInfo | null;
  /** One sentence saying what is currently true, including the states that look like breakage. */
  reason: string | null;
}

/** One tick of a dictation. `committed` never shrinks, so `committed + tentative` never flickers. */
export interface DictationUpdate {
  kind: "started" | "text" | "recording" | "finished" | "failed";
  committed: string;
  tentative: string;
  recordedMs: number;
  device: string | null;
  /** Whether text appears as you speak; false means it arrives when you stop. */
  streaming: boolean;
  text: string | null;
  error: string | null;
}

export interface RerankSetting {
  enabled: boolean;
  /** Currently only in-process ONNX Runtime is supported. */
  backend: "onnx";
  /** Fixed multilingual cross-encoder shipped with the app. */
  model: string;
  /** How many chunks to fetch for rescoring; this is the latency dial. */
  candidates: number;
  /** How many to keep after scoring. */
  topN: number;
  /** A sentence naming the cost being paid: how much slower, or what is lost when off. */
  reason: string | null;
}

/** The effective embedding configuration. */
export interface EmbeddingSetting {
  providerId: string | null;
  providerName: string | null;
  model: string | null;
  /** Documents never leave this machine during embedding. */
  onDevice: boolean;
  reason: string | null;
}

/** Result of actually embedding one sentence; listing models proves nothing about which of them can embed. */
export interface EmbeddingProbe {
  ok: boolean;
  message: string;
  /** Dimensions measured from the real vector. */
  dimensions: number | null;
}

/** The effective vision configuration: who reads images for OCR, and whether OCR is even on. */
export interface VisionSetting {
  providerId: string | null;
  providerName: string | null;
  model: string | null;
  /** Page images never leave this machine while being read. */
  onDevice: boolean;
  reason: string | null;
  /** The OCR switch; off means images and scanned pages are skipped rather than read. */
  ocrEnabled: boolean;
  /** Whether pictures inside pages that already have text are read too — the optional half of OCR. */
  ocrImages: boolean;
}

/** Result of really reading the bundled test image; a model list never says which models can see. */
export interface VisionProbe {
  ok: boolean;
  message: string;
  /** What the model answered, so a wrong read reads as a wrong read. */
  text: string | null;
}

export interface ProviderPreset {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  needsKey: boolean;
  onDevice: boolean;
  defaultModel: string | null;
  homepage: string;
  hint: string;
}

/** Sent on save; `apiKey === null` keeps the stored key rather than clearing it. */
export interface ProviderInput {
  id: string | null;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  apiKey: string | null;
  enabled: boolean;
  model: string | null;
  embeddingModel: string | null;
  visionModel: string | null;
}

/** One entry in the project directory tree. */
export interface DirEntry {
  name: string;
  /** Absolute path, sent back verbatim when expanding a subdirectory. */
  path: string;
  isDir: boolean;
}

export interface ProviderProbe {
  ok: boolean;
  message: string;
  models: ModelChoice[];
}

export type McpState = "connected" | "connecting" | "failed" | "disabled";

export interface McpServer {
  name: string;
  transport: "stdio" | "http";
  target: string;
  enabled: boolean;
  state: McpState;
  tools: string[];
  error: string | null;
}

export interface McpEnvVar {
  key: string;
  label: string;
  required: boolean;
  secret: boolean;
}

export interface McpCatalogEntry {
  id: string;
  name: string;
  summary: string;
  command: string;
  args: string[];
  env: McpEnvVar[];
  homepage: string;
  /** `node`, `python`, `docker`: warn up front instead of letting the user stare at `failed`. */
  requires: string[];
  /** Endpoint of a *remotely hosted* server; `null` means local. With it, `requires` is empty because nothing is needed. */
  url: string | null;
}

export interface McpServerInput {
  name: string;
  transport: "stdio" | "http";
  command: string;
  args: string[];
  env: Record<string, string>;
  cwd: string | null;
  url: string;
  headers: Record<string, string>;
  enabled: boolean;
}

export type GraphNodeKind =
  | "function"
  | "method"
  | "struct"
  | "class"
  | "trait"
  | "interface"
  | "enum"
  | "module"
  | "constant"
  | "type";

export interface GraphNode {
  id: string;
  name: string;
  kind: GraphNodeKind;
  path: string;
  line: number;
}

export type GraphEdgeKind =
  | "calls"
  | "imports"
  | "contains"
  | "implements"
  | "extends"
  | "references";

export interface GraphEdge {
  src: string;
  dst: string;
  kind: GraphEdgeKind;
}

export interface GraphView {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** Truncated to stay drawable: a node with four hundred edges renders as a black ball. */
  truncated: boolean;
}

export interface IndexStats {
  files: number;
  symbols: number;
  edges: number;
  languages: [string, number][];
  scannedAt: number | null;
}
