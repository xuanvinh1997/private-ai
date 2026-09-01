/**
 * Bản sao TypeScript của `AgentEvent` trong app/src/lib.rs.
 *
 * Hai đầu phải khớp bằng tay cho tới khi có bước sinh mã (ts-rs hoặc specta). Cho tới
 * lúc đó, sửa một bên mà quên bên kia sẽ hỏng lúc chạy chứ không lúc biên dịch — nên
 * mọi thay đổi ở đây phải kèm thay đổi tương ứng bên Rust trong cùng một commit.
 *
 * Phần dưới `Error` là những biến thể giao diện cần nhưng Rust CHƯA có. Chúng được
 * khai báo trước để dựng được giao diện và để khoá hình dạng payload; danh sách đầy đủ
 * nằm trong báo cáo bàn giao. Trên wire, tên trường giữ snake_case vì `rename_all` của
 * serde chỉ đổi tên *biến thể*, không đổi tên trường.
 */

/** Một hunk diff. `old_text: null` nghĩa là tệp mới — không phải là "không đổi". */
export interface DiffHunk {
  path: string;
  old_text: string | null;
  new_text: string;
  /**
   * Số dòng đầu của phần cũ/mới trong tệp thật. Bên dsh không mang số này qua wire nên
   * nó là tuỳ chọn: vắng thì khối diff đánh số từ 1 và đó là số *trong hunk*, không
   * phải số trong tệp. Nếu Rust tính được thì gửi kèm, giao diện dùng ngay.
   */
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

/**
 * `tool/result.data.meta` — thứ duy nhất được transport để giao diện vẽ thẻ giàu.
 *
 * dsh có `presentCall`/`presentResult` phía host nhưng bản web KHÔNG dùng; nó đọc thẳng
 * `meta`. Ta chép đúng chỗ đó: giao diện tự render từ sự kiện thô, không có API trình bày.
 */
export interface ToolMeta {
  diffs?: DiffHunk[];
  read?: {
    path: string;
    offset: number;
    lines: ReadLine[];
    total_lines: number;
    lang?: string | null;
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
    /** Lệnh chạy nền: chưa có exit code không có nghĩa là treo. */
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

/** Quyết định duyệt. Chỉ hai giá trị: không có "nhớ lựa chọn" trong từ vựng. */
export type ApprovalDecision = "allowed_once" | "rejected";

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
      /** CHƯA có bên Rust. Nguồn của mọi thẻ giàu (diff, terminal, read, search). */
      meta?: ToolMeta | null;
    }
  | { kind: "final"; message_id: string }
  | { kind: "error"; message: string }
  /**
   * CHƯA có bên Rust — diff *dự kiến*, phát ngay khi tool bắt đầu để người dùng thấy
   * thay đổi trước khi nó xảy ra. Giao diện cũng tự suy được từ `args` (xem `diff.ts`),
   * nên biến thể này chỉ là đường tắt cho tool mà args không đủ để dựng diff.
   */
  | { kind: "diff"; call_id: string; diffs: DiffHunk[] }
  /**
   * CHƯA có bên Rust — danh sách việc. dsh đi qua projection `todos`, KHÔNG derive từ
   * event; ta giữ đúng tinh thần đó bằng một sự kiện mang *toàn bộ* danh sách mỗi lần,
   * để giao diện không phải gấp trạng thái.
   */
  | { kind: "todo"; items: TodoItem[] }
  /**
   * CHƯA có bên Rust — host hỏi ngược giao diện. Tương ứng waterfall `approval/request`
   * của dsh. Không trả lời được = từ chối (xem `agent.ts`).
   */
  | {
      kind: "approval_request";
      request_id: string;
      call_id: string;
      name: string;
      args: unknown;
      reason: string | null;
      timeout_ms: number | null;
    }
  /** CHƯA có bên Rust — host rút lại câu hỏi (lượt bị huỷ). Giao diện đóng hộp thoại. */
  | { kind: "approval_cancel"; request_id: string };

/** Một tool call đang chạy hoặc đã xong, dựng từ cặp tool_start/tool_end. */
export interface ToolCall {
  callId: string;
  name: string;
  args: unknown;
  state: "running" | "ok" | "error";
  preview?: string;
  meta?: ToolMeta;
  /** Diff dự kiến lúc đang chạy. Bị `meta.diffs` thay thế khi tool xong. */
  intendedDiffs?: DiffHunk[];
}

/**
 * Một dòng trong bản ghi hội thoại.
 *
 * Đây là đơn vị dispatch của registry: `kind` là khoá tra renderer. Thêm một loại nội
 * dung mới = thêm một biến thể ở đây + một lần `registerNode`, không đụng `Transcript`.
 */
export type ConversationNode =
  | { id: string; kind: "user"; text: string; at?: number }
  | { id: string; kind: "assistant"; text: string; streaming: boolean; at?: number }
  | { id: string; kind: "tool"; call: ToolCall; at?: number }
  | { id: string; kind: "notice"; message: string }
  | { id: string; kind: "progress"; label: string; detail: string | null }
  | { id: string; kind: "error"; message: string }
  | { id: string; kind: "todo"; items: TodoItem[] };

export type NodeKind = ConversationNode["kind"];

/** Một phiên trong thanh bên. `updatedAt` là epoch ms. */
export interface SessionSummary {
  id: string;
  title: string;
  /**
   * Câu cuối cùng đã nói trong phiên. `null` khi phiên chưa nói gì — và lúc đó hàng phải
   * là **một dòng**, không phải hai dòng với dòng dưới trống.
   */
  preview: string | null;
  updatedAt: number;
}

/**
 * Một node trong bản ghi đã lưu, dựng lại từ sổ tay phiên bên Rust.
 *
 * Cùng từ vựng `kind` với `ConversationNode` để sổ đăng ký renderer dùng lại nguyên vẹn —
 * bản ghi nạp lại và lượt đang chạy vẽ bằng cùng một mã.
 *
 * `created_at` giữ snake_case vì `serde` chỉ đổi tên *biến thể*, không đổi tên trường.
 * Nó là **giờ trong sổ**, khác với giờ mà lượt đang chạy tự đóng dấu lúc nhận — hai
 * nguồn khác nhau cho hai tình huống khác nhau, và cái đó đúng.
 */
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

/** Một mô hình máy chủ đang có. */
export interface ModelChoice {
  id: string;
  /**
   * Gọi được tool không. Mô hình không gọi được tool thì coding agent im lặng vô dụng —
   * nó vẫn trả lời, chỉ là không bao giờ đọc hay sửa được gì — nên giao diện phải nói ra
   * trước khi người dùng chọn nhầm.
   */
  tools: boolean;
  /** Trò chuyện được. */
  chat: boolean;
  /**
   * Nhúng được.
   *
   * Hai cờ không loại trừ nhau. Chỉ nhóm `embedding && !chat` — thứ **chỉ** nhúng được —
   * bị giấu khỏi bộ chọn mô hình hội thoại. Lọc theo `chat` thì chặt hơn nhưng sai hướng:
   * một máy chủ Ollama đời cũ không có trường `capabilities` buộc lõi đoán theo tên, và
   * một lần đoán trượt khi ấy làm biến mất một mô hình đang dùng được.
   */
  embedding: boolean;
  contextWindow: number | null;
}

/**
 * Một dự án trong danh sách gần đây.
 *
 * `isCurrent` do lõi chấm chứ không do giao diện suy ra: đổi dự án là tháo và cắm lại cả
 * một nhánh plugin, nên chỉ lõi mới biết nhánh nào đã cắm xong. Giao diện đoán trước sẽ
 * hiện tên dự án mới trong lúc mọi tool vẫn còn trỏ vào dự án cũ.
 */
export interface Project {
  id: string;
  name: string;
  path: string;
  lastOpenedAt: number;
  isCurrent: boolean;
  kind: ProjectKind;
  /** URL đã clone về. `null` = thư mục vốn có sẵn trên máy. */
  origin: string | null;
}

/* ─────────────────────────────────────────────────────────────────────────────
 * Dự án hai loại, thư viện tài liệu, provider, và MCP.
 *
 * Bản gốc nằm ở `app/src/protocol.rs`; hai đầu khớp bằng tay, nên sửa ở đây phải sửa cả
 * bên kia trong cùng một commit.
 * ───────────────────────────────────────────────────────────────────────────── */

/**
 * Mã nguồn, hay một chồng tài liệu.
 *
 * Không phải một nhãn để lọc: nó chọn tầng plugin nào được cắm. Dự án tài liệu không có
 * `bash` và không có `edit` — người ta không sửa mã trong một thư mục toàn PDF, và cấp
 * quyền chạy lệnh cho một chỗ toàn tệp người ngoài gửi tới là mở đúng cánh cửa không nên
 * mở.
 */
export type ProjectKind = "code" | "docs";

/** Tiến trình `git clone`. `percent` vắng ở những pha git không đếm được. */
export interface CloneProgress {
  phase: string;
  percent: number | null;
  line: string | null;
  finished: boolean;
  path: string | null;
  error: string | null;
}

export type DocumentFormat = "pdf" | "docx" | "markdown" | "text" | "html" | "csv" | "code";

/** Một tài liệu trong thư viện. */
export interface DocumentView {
  id: string;
  path: string;
  title: string;
  format: DocumentFormat;
  bytes: number;
  chunks: number;
  /**
   * Đã có vector chưa. `false` mà `error` là `null` nghĩa là **đang xếp hàng**, không
   * phải hỏng — và giao diện phải nói đúng như vậy, vì tìm bằng từ khoá đã chạy được rồi.
   */
  embedded: boolean;
  addedAt: number;
  error: string | null;
}

export interface IngestProgress {
  path: string;
  /** `reading` `stored` `failed` `skipped` `removed` `finished`. */
  stage: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
}

/** Sức khoẻ thư viện — đủ để nói **vì sao** câu trả lời kém, thay vì chỉ nói nó kém. */
export interface LibraryStats {
  documents: number;
  chunks: number;
  embeddedChunks: number;
  embedder: string | null;
  semanticReady: boolean;
  reason: string | null;
  /**
   * Thư mục tài liệu của người dùng.
   *
   * Màn hình phải chỉ ra được nó: câu hỏi "vì sao không thấy tệp nào" bắt đầu bằng việc
   * người dùng kiểm lại họ đã chỉ vào đâu.
   */
  root: string;
  filesSeen: number;
  /** Bỏ qua vì chạm trần — kích thước tệp hoặc trần số tệp. */
  filesSkipped: number;
  unreadable: number;
  /** Còn trong thư mục nhưng người dùng đã bỏ khỏi thư viện. */
  excluded: number;
  /** `null` là **chưa quét lần nào** — khác hẳn "quét rồi và không có gì". */
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

export type ProviderKind = "ollama" | "openai";

/**
 * Một provider đã cấu hình. **Không bao giờ mang khoá API.**
 *
 * `hasKey` thay cho chính cái khoá: giao diện chỉ cần biết ô nhập nên hiện "đã đặt" hay
 * hiện trống. Một khoá đi qua IPC là một khoá nằm sẵn trong mọi công cụ gỡ lỗi đang mở.
 */
export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  hasKey: boolean;
  enabled: boolean;
  onDevice: boolean;
  /** Đang dùng để **trò chuyện**. */
  activeChat: boolean;
  /**
   * Đang dùng để **nhúng tài liệu**.
   *
   * Hai vai tách hẳn nhau, và không phải để cho có: mô hình nhúng và mô hình hội thoại là
   * hai loại mô hình khác nhau trên hai endpoint khác nhau, và cách ghép hợp lý nhất
   * trong thực tế lại là ghép chéo — nhúng bằng một mô hình nhỏ chạy tại chỗ, trò chuyện
   * bằng một mô hình lớn từ xa. Buộc chúng dùng chung một provider là loại bỏ đúng cấu
   * hình mà phần lớn người dùng muốn.
   */
  activeEmbedding: boolean;
  /** Mô hình hội thoại. */
  model: string | null;
  /** Mô hình nhúng. */
  embeddingModel: string | null;
}

/** Cấu hình nhúng đang có hiệu lực. */
export interface EmbeddingSetting {
  providerId: string | null;
  providerName: string | null;
  model: string | null;
  /** Tài liệu không rời khỏi máy này khi nhúng. */
  onDevice: boolean;
  reason: string | null;
}

/**
 * Kết quả thử **nhúng thật một câu**.
 *
 * Không phải một phép liệt kê mô hình: `/api/tags` trả về mọi mô hình và không có gì
 * trong đó nói cái nào nhúng được. Cách duy nhất biết chắc là gửi một câu đi và xem có
 * vector trả về không.
 */
export interface EmbeddingProbe {
  ok: boolean;
  message: string;
  /** Số chiều đo được từ vector thật. */
  dimensions: number | null;
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

/** Gửi lên khi lưu. `apiKey` là `null` nghĩa là **giữ nguyên khoá cũ**, không phải xoá. */
export interface ProviderInput {
  id: string | null;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  apiKey: string | null;
  enabled: boolean;
  model: string | null;
  embeddingModel: string | null;
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
  /** `node`, `python`, `docker` — cảnh báo trước, chứ không để người dùng nhìn `failed`. */
  requires: string[];
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
  /** Đã cắt bớt để vẽ được: một đỉnh bốn trăm cạnh vẽ ra là một quả cầu đen. */
  truncated: boolean;
}

export interface IndexStats {
  files: number;
  symbols: number;
  edges: number;
  languages: [string, number][];
  scannedAt: number | null;
}
