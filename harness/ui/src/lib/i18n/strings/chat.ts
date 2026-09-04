import type { Msg } from "../core";
import { common } from "./common";

/** Strings for the `chat` area. See lib/i18n/README.md for the wording rules. */
export const chat = {
  // Used by both the sidebar and the command palette: one phrase per action.
  sessionSearch: { en: "Search sessions…", vi: "Tìm phiên…" },
  noSessionMatch: { en: "No session matches.", vi: "Không có phiên nào khớp." },

  // ---- Composer -------------------------------------------------------------
  composer: {
    // Three permission levels, never shortened: this is what the assistant is allowed to do.
    scopeRead: common.scopeRead,
    scopeWrite: common.scopeWrite,
    scopeShell: common.scopeShell,
    scopeMenu: { en: "Tool scope: {scope}", vi: "Phạm vi tool: {scope}" },
    scopeIdle: { en: "· unavailable", vi: "· chưa dùng được" },

    placeholder: {
      en: "Message…  (Enter to send, Shift+Enter for a new line)",
      vi: "Nhập…  (Enter để gửi, Shift+Enter xuống dòng)",
    },
    placeholderBusy: {
      en: "Next message…  (Enter to queue it)",
      vi: "Gõ câu tiếp theo…  (Enter để xếp hàng chờ)",
    },
    field: { en: "Message", vi: "Nội dung tin nhắn" },

    queued: { en: "Sends when done", vi: "Gửi khi xong" },
    unqueue: { en: "Drop the queued message", vi: "Bỏ câu đang chờ" },

    attach: { en: "Attach files, or drop them here", vi: "Đính kèm tệp, hoặc kéo thả vào cửa sổ" },
    send: common.sendMessage,
    stop: { en: "Stop the running turn", vi: "Dừng lượt đang chạy" },

    noPathMatch: { en: "No file matches.", vi: "Không có tệp nào khớp." },
    needsProject: { en: "needs a project", vi: "cần một dự án" },
    noProject: { en: "No project: no tools, still sends.", vi: "Chưa có dự án: chưa có tool, vẫn gửi được." },

    // Status line under the composer.
    metaLabel: { en: "Next turn runs with", vi: "Lượt kế sẽ chạy với" },
    kindDocs: { en: "docs", vi: "tài liệu" },
    kindCode: { en: "code", vi: "mã nguồn" },
    mcpOne: { en: "{n} MCP server", vi: "{n} server MCP" },
    mcpMany: { en: "{n} MCP servers", vi: "{n} server MCP" },
    context: { en: "Context {n}%", vi: "Ngữ cảnh {n}%" },

    // Attachment errors: they must name what broke, so they are not shortened.
    attachNoProject: {
      en: "Open a project before attaching files.",
      vi: "Chưa mở dự án nên chưa đính kèm tệp được.",
    },
    attachNoPicker: {
      en: "The file picker only exists in the app.",
      vi: "Hộp thoại chọn tệp chỉ có trong ứng dụng.",
    },
    attachPickerFailed: common.pickerFailed,
    attachRefusedMore: {
      en: "{err} (and {n} more files were not attached)",
      vi: "{err} (và {n} tệp nữa không đính kèm được)",
    },
  },

  // ---- Sidebar --------------------------------------------------------------
  sidebar: {
    nav: common.navigation,
    navMain: { en: "Main navigation", vi: "Điều hướng chính" },
    collapse: { en: "Collapse the sidebar", vi: "Thu gọn thanh bên" },

    searchOpen: { en: "Find a session", vi: "Tìm phiên" },
    searchClose: { en: "Close the session filter", vi: "Đóng ô tìm phiên" },
    searchField: { en: "Search sessions by name", vi: "Tìm phiên theo tên" },

    newSession: common.newSession,
    mcp: common.mcpServers,
    projects: common.projects,
    recent: { en: "Recent", vi: "Gần đây" },
    noSessions: { en: "No sessions yet.", vi: "Chưa có phiên nào." },

    rowMenu: { en: "Options for {title}", vi: "Tuỳ chọn cho {title}" },
    deleteSession: common.deleteSession,

    // Tab label of the open project.
    tabChanges: { en: "Changes", vi: "Thay đổi" },
    tabDocs: { en: "Docs", vi: "Thư viện tài liệu" },

    themeLight: { en: "Light", vi: "Giao diện sáng" },
    themeDark: { en: "Dark", vi: "Giao diện tối" },
    themeSystem: common.themeSystem,
    themeToggle: { en: "{name} theme. Click to switch.", vi: "{name}. Bấm để đổi." },
  },

  // ---- Workspace top bar ----------------------------------------------------
  header: {
    openSidebar: { en: "Show the sidebar", vi: "Hiện thanh bên" },
    busy: { en: "running…", vi: "đang chạy…" },
    openChanges: { en: "Open the changes panel", vi: "Mở bảng thay đổi" },
    openChangesCount: {
      en: "Open the changes panel, {n} files",
      vi: "Mở bảng thay đổi, {n} tệp",
    },
  },

  // ---- Right inspector ------------------------------------------------------
  inspector: {
    label: { en: "Workspace inspector", vi: "Bảng thông tin workspace" },
    tabs: { en: "Workspace inspector views", vi: "Các góc nhìn của workspace" },
    close: { en: "Close the workspace inspector", vi: "Đóng bảng thông tin workspace" },
  },

  // ---- Session search palette -----------------------------------------------
  palette: {
    title: common.findSession,
    field: common.sessionName,
    results: { en: "Results", vi: "Kết quả" },
    current: { en: "open", vi: "đang mở" },
  },

  // ---- Changes panel --------------------------------------------------------
  changes: {
    title: { en: "Changed files", vi: "Tệp đã thay đổi" },
    close: { en: "Close the changes panel", vi: "Đóng bảng thay đổi" },
    empty: { en: "No file touched yet.", vi: "Phiên này chưa đụng vào tệp nào." },
    created: { en: "new", vi: "tệp mới" },
    pending: common.planned,
    reveal: {
      en: "Show where {name} was edited in the transcript",
      vi: "Xem lúc trợ lý sửa {name} trong bản ghi",
    },
    fileOne: { en: "{n} file", vi: "{n} tệp" },
    fileMany: { en: "{n} files", vi: "{n} tệp" },
  },

  // ---- Toasts ---------------------------------------------------------------
  toast: {
    close: { en: "Dismiss this notice", vi: "Đóng thông báo" },
  },

  // ---- Model picker ---------------------------------------------------------
  model: {
    trigger: { en: "Model: {name}. Click to change.", vi: "Mô hình: {name}. Bấm để đổi." },
    noServer: {
      en: "No model server answered yet.",
      vi: "Chưa hỏi được máy chủ mô hình nào.",
    },
    embedOnly: {
      en: "Embedding models only, no chat.",
      vi: "Chỉ có mô hình nhúng, không trò chuyện được.",
    },
    hasTools: { en: "calls tools", vi: "gọi được công cụ" },
    noTools: { en: "no tool calls", vi: "không gọi được công cụ" },
    context: { en: "· {n}K context", vi: "· {n}K ngữ cảnh" },
    providers: { en: "Model providers…", vi: "Nhà cung cấp mô hình…" },
  },

  // ---- Empty state ----------------------------------------------------------
  empty: {
    readyTitle: { en: "Start chatting", vi: "Trò chuyện được ngay" },
    readyBody: { en: "No project needed to chat.", vi: "Chưa có dự án, trợ lý vẫn trả lời được." },
    limitBody: { en: "Nothing on disk is touched.", vi: "Trợ lý chưa đọc, sửa hay chạy gì trên máy." },
    limitInfo: { en: "About the limits without a project", vi: "Về giới hạn khi chưa có dự án" },
    limitInfoBody: {
      en: "Without a project the assistant reads nothing, edits nothing and runs nothing on this machine. Opening a project points it at exactly one folder to work in.",
      vi: "Chưa có dự án thì trợ lý không đọc, không sửa và không chạy được gì trên máy này. Mở một dự án là chỉ cho nó đúng một thư mục để làm việc.",
    },
    openProject: { en: "Open a project", vi: "Mở một dự án" },

    title: { en: "What's next?", vi: "Ta làm gì hôm nay?" },
    codeBody: { en: "Reads, edits and runs here.", vi: "Trợ lý đọc, sửa tệp và chạy lệnh ở đây." },
    codeInfo: { en: "About rights in a code project", vi: "Về quyền trong dự án mã nguồn" },
    codeInfoBody: {
      en: "The assistant reads and edits files inside the working folder, runs commands, and asks first before every write.",
      vi: "Trợ lý đọc và sửa được tệp trong thư mục làm việc, chạy được lệnh, và hỏi lại trước mỗi thao tác ghi.",
    },
    docsBody: { en: "Answers from docs, with sources.", vi: "Trợ lý đọc tài liệu để trả lời, kèm nguồn." },
    docsInfo: { en: "About rights in a docs library", vi: "Về quyền trong thư viện tài liệu" },
    docsInfoBody: {
      en: "The assistant searches and reads documents in this library to answer, and says where each answer came from. It edits no file and runs no command in this kind of project.",
      vi: "Trợ lý tìm và đọc tài liệu trong thư viện này để trả lời, kèm chỗ nó lấy ra. Nó không sửa tệp và không chạy lệnh trong dự án loại này.",
    },
  },

  // ---- Approval dialog ------------------------------------------------------
  // Never shortened here: the reader must know what the assistant is about to do.
  approval: {
    title: { en: "Allow {tool} to run?", vi: "Cho phép chạy {tool}?" },
    body: { en: "The assistant wants to call {name}.", vi: "Trợ lý muốn gọi {name}." },
    timeout: {
      en: "No answer in {n} seconds rejects this automatically.",
      vi: "Không trả lời trong {n} giây thì tự từ chối.",
    },
    reject: { en: "Reject", vi: "Từ chối" },
    allowOnce: { en: "Allow once", vi: "Cho phép một lần" },
  },

  // ---- Working indicator ----------------------------------------------------
  thinking: {
    idle: { en: "Thinking", vi: "Đang suy nghĩ" },
    running: { en: "Running {name}", vi: "Đang chạy {name}" },
    doing: { en: "Running {what}", vi: "Đang {what}" },
  },

  // ---- Todo list ------------------------------------------------------------
  todo: {
    title: { en: "To-dos", vi: "Danh sách việc" },
    empty: { en: "Nothing here yet.", vi: "Chưa có việc nào." },
    statusPending: { en: "to do", vi: "chưa làm" },
    statusRunning: { en: "in progress", vi: "đang làm" },
    statusDone: { en: "done", vi: "xong" },
    statusCancelled: { en: "cancelled", vi: "bỏ" },
  },

  // ---- Transcript -----------------------------------------------------------
  transcript: {
    toBottom: { en: "Latest", vi: "Về cuối" },
    unknown: { en: "(no view for {kind} yet)", vi: "(chưa có cách hiển thị cho {kind})" },
  },

  // ---- Diff block -----------------------------------------------------------
  diff: {
    captionOne: { en: "Changes ({n} file)", vi: "Thay đổi ({n} tệp)" },
    captionMany: { en: "Changes ({n} files)", vi: "Thay đổi ({n} tệp)" },
    collapse: { en: "Collapse", vi: "Gập lại" },
    expand: { en: "Expand ({n} lines)", vi: "Mở rộng ({n} dòng)" },
    copy: { en: "Copy the diff as unified", vi: "Chép diff dạng unified" },
    lineAdded: { en: "added line: {text}", vi: "dòng thêm: {text}" },
    lineRemoved: { en: "removed line: {text}", vi: "dòng xoá: {text}" },
  },
} satisfies Record<string, Msg | Record<string, Msg>>;
