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
}

/**
 * Một mục trong cây tệp.
 *
 * `children` **vắng mặt** nghĩa là chưa nạp, `[]` nghĩa là thư mục rỗng. Hai thứ đó khác
 * nhau: gộp lại thì một thư mục chưa mở và một thư mục không có gì trông giống hệt nhau,
 * và cây sẽ không bao giờ nạp cấp tiếp theo.
 */
export interface TreeEntry {
  path: string;
  name: string;
  isDir: boolean;
  children?: TreeEntry[];
}

/**
 * Nội dung một tệp để xem.
 *
 * `truncated` là một phần của hợp đồng chứ không phải chi tiết cài đặt: một tệp bị cắt
 * mà không nói ra thì người đọc kết luận "hết rồi" ở đúng chỗ lõi ngừng đọc.
 */
export interface FileView {
  text: string;
  lang: string | null;
  totalLines: number;
  truncated: boolean;
}
