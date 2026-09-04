import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `tools` area. See lib/i18n/README.md for the wording rules. */
export const tools = {
  /** Tool call states; screen readers read these verbatim from `aria-label`, so they are never shortened. */
  state: {
    running: { en: "running", vi: "đang chạy" },
    ok: { en: "done", vi: "xong" },
    warn: { en: "done, with a warning", vi: "xong, có cảnh báo" },
    error: { en: "failed", vi: "lỗi" },
  },

  // --- Shared transcript pieces: copy button, openable paths ---
  copy: {
    content: { en: "Copy", vi: "Chép nội dung" },
    failed: { en: "Could not copy. Try again.", vi: "Không chép được nội dung. Thử lại." },
  },
  openFile: { en: "Open {path}", vi: "Mở {path}" },
  openFileAt: { en: "Open {path} at line {n}", vi: "Mở {path} ở dòng {n}" },

  /** Labels for wire tool names; an unknown name is shown verbatim. */
  name: {
    read: { en: "Read file", vi: "Đọc tệp" },
    write: { en: "Write file", vi: "Ghi tệp" },
    edit: { en: "Edit file", vi: "Sửa tệp" },
    glob: { en: "Find files", vi: "Tìm tệp" },
    grep: { en: "Search files", vi: "Tìm trong tệp" },
    bash: { en: "Run command", vi: "Chạy lệnh" },
    jobOutput: { en: "Job output", vi: "Đầu ra tiến trình" },
    jobKill: { en: "Stop job", vi: "Dừng tiến trình" },
    todoWrite: { en: "Task list", vi: "Danh sách việc" },
  },

  // --- Shared frame of every tool card ---
  card: {
    aria: { en: "{name} — {state}", vi: "{name} — {state}" },
    args: { en: "Arguments", vi: "Đối số" },
    result: { en: "Result", vi: "Kết quả" },
    chars: { en: "{n} chars", vi: "{n} ký tự" },
  },

  bash: {
    hidden: common.linesHidden,
    background: { en: "background", vi: "nền" },
    signal: { en: "signal {name}", vi: "tín hiệu {name}" },
    cwd: { en: "in {path}", vi: "tại {path}" },
    output: { en: "Output", vi: "Đầu ra" },
    copyOutput: { en: "Copy command output", vi: "Chép đầu ra lệnh" },
    expand: { en: "Show the whole output", vi: "Hiện toàn bộ đầu ra" },
    collapse: { en: "Collapse the output", vi: "Gập đầu ra lại" },
    jobless: {
      en: "Runs in background — read it with job_output.",
      vi: "Chạy nền — dùng job_output để xem.",
    },
    job: {
      en: "Background job {id} — read it with job_output.",
      vi: "Chạy nền · mã tiến trình {id} — dùng job_output để xem.",
    },
  },

  read: {
    lines: { en: "{n}/{total} lines", vi: "{n}/{total} dòng" },
    fromLine: { en: "from line {n}", vi: "từ dòng {n}" },
    content: { en: "Content", vi: "Nội dung" },
    oneLine: { en: "{n} line", vi: "{n} dòng" },
    manyLines: common.linesMany,
  },

  search: {
    oneMatch: { en: "{n} match", vi: "{n} khớp" },
    manyMatches: { en: "{n} matches", vi: "{n} khớp" },
    oneFile: { en: "{n} file", vi: "{n} tệp" },
    manyFiles: { en: "{n} files", vi: "{n} tệp" },
    truncated: { en: "truncated", vi: "đã cắt bớt" },
    more: { en: "{n} more matches in this file", vi: "còn {n} khớp nữa trong tệp này" },
    paths: { en: "Paths", vi: "Đường dẫn" },
  },

  mutation: {
    intended: common.planned,
  },

  todo: {
    oneTask: { en: "{n} task", vi: "{n} việc" },
    manyTasks: { en: "{n} tasks", vi: "{n} việc" },
  },

  // --- Messages in the transcript ---
  message: {
    you: { en: "You", vi: "Bạn" },
    assistant: { en: "Assistant", vi: "Trợ lý" },
    copy: { en: "Copy message", vi: "Chép tin nhắn" },
    copyReply: { en: "Copy reply", vi: "Chép câu trả lời" },
    resend: { en: "Send again", vi: "Gửi lại" },
    remove: { en: "Remove from transcript", vi: "Xoá khỏi bản ghi" },
  },

  // --- Fenced code blocks ---
  code: {
    streaming: { en: "receiving", vi: "đang nhận" },
    copy: { en: "Copy code block", vi: "Chép khối mã" },
    plain: { en: "code", vi: "mã" },
    text: { en: "text", vi: "văn bản" },
  },

  // --- Mermaid diagrams and math ---
  diagram: {
    title: { en: "Diagram", vi: "Sơ đồ" },
    views: { en: "Diagram view", vi: "Cách xem sơ đồ" },
    figure: { en: "Figure", vi: "Hình" },
    source: { en: "Source", vi: "Mã nguồn" },
    copySource: { en: "Copy diagram source", vi: "Chép mã nguồn sơ đồ" },
    drawing: { en: "Drawing diagram…", vi: "Đang vẽ sơ đồ…" },
    alt: {
      en: '{kind} drawn by the assistant — switch to the "{source}" view to read it as text',
      vi: '{kind} do trợ lý vẽ — chuyển sang lối xem "{source}" để đọc bằng chữ',
    },
  },
  /** Prefix for a mermaid or KaTeX error message; not shortened, since it reports a failure. */
  renderFailed: { en: "Cannot render:", vi: "Không vẽ được:" },
} satisfies Record<string, Msg | Record<string, Msg>>;
